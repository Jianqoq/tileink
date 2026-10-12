//! Transaction application; scene storage and rollback stay in `scene`.
use super::model::*;
use super::scene::RetainedScene;
use super::transaction::{Mutation, UndoMutation};
use super::validation::*;

impl RetainedScene {
    #[allow(
        clippy::expect_used,
        reason = "Mutation validation establishes node presence and parentage before applying changes"
    )]
    pub(super) fn apply_with_undo(
        &mut self,
        mutation: Mutation,
        changes: &mut SceneChangeSet,
    ) -> Result<UndoMutation, RetainedSceneError> {
        match mutation {
            Mutation::Insert {
                parent,
                before,
                id,
                kind,
            } => {
                if self.nodes.contains_key(&id) {
                    return Err(RetainedSceneError::DuplicateNode(id));
                }
                validate_kind(&kind, self.scale)?;
                let insertion = self.insert_child(parent, before, id)?;
                record_rebalanced_siblings(&insertion, changes);
                let instance = self.next_node_instance;
                self.next_node_instance = self.next_node_instance.wrapping_add(1).max(2);
                self.nodes.insert(
                    id,
                    SceneNode {
                        kind,
                        parent: Some(parent),
                        content: ChildList::default(),
                        mask: ChildList::default(),
                        instance,
                        generation: 0,
                    },
                );
                changes.changed_nodes.insert(id);
                changes.topology_changed = true;
                changes.hierarchy_changed = true;
                Ok(UndoMutation::Insert {
                    parent,
                    id,
                    insertion,
                })
            }
            Mutation::ReplaceScene { id, canvas } => {
                validate_canvas(&canvas, self.scale)?;
                let node = self
                    .nodes
                    .get_mut(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?;
                let NodeKind::Scene {
                    transform,
                    translation_damage,
                    ..
                } = &node.kind
                else {
                    return Err(RetainedSceneError::MissingNode(id));
                };
                let old_kind = node.kind.clone();
                let old_generation = node.generation;
                node.kind = NodeKind::Scene {
                    canvas,
                    transform: *transform,
                    translation_damage: *translation_damage,
                };
                node.generation = node.generation.wrapping_add(1);
                changes.changed_nodes.insert(id);
                Ok(UndoMutation::NodeValue {
                    id,
                    kind: old_kind,
                    generation: old_generation,
                })
            }
            Mutation::SetTransform {
                id,
                transform,
                translation_damage,
            } => {
                validate_transform(transform)?;
                if let Some(damage) = translation_damage {
                    validate_damage_rect(damage)?;
                }
                let node = self
                    .nodes
                    .get_mut(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?;
                let NodeKind::Scene {
                    canvas: current_canvas,
                    transform: current,
                    translation_damage: current_damage,
                } = &node.kind
                else {
                    return Err(RetainedSceneError::MissingNode(id));
                };
                if translation_damage.is_some() && !affine_linear_part_eq(*current, transform) {
                    return Err(RetainedSceneError::InvalidTransform);
                }
                if *current != transform || *current_damage != translation_damage {
                    let old_kind = node.kind.clone();
                    let old_generation = node.generation;
                    node.kind = NodeKind::Scene {
                        canvas: std::rc::Rc::clone(current_canvas),
                        transform,
                        translation_damage,
                    };
                    node.generation = node.generation.wrapping_add(1);
                    changes.changed_nodes.insert(id);
                    Ok(UndoMutation::NodeValue {
                        id,
                        kind: old_kind,
                        generation: old_generation,
                    })
                } else {
                    Ok(UndoMutation::None)
                }
            }
            Mutation::UpdateLayer { id, layer } => {
                validate_layer(&layer)?;
                let node = self
                    .nodes
                    .get_mut(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?;
                if !matches!(node.kind, NodeKind::Layer(_)) {
                    return Err(RetainedSceneError::MissingNode(id));
                }
                if !matches!(layer, RetainedLayerDescriptor::Mask(_)) && !node.mask.is_empty() {
                    return Err(RetainedSceneError::InvalidParentBranch(id));
                }
                let old_kind = node.kind.clone();
                let old_generation = node.generation;
                node.kind = NodeKind::Layer(layer);
                node.generation = node.generation.wrapping_add(1);
                changes.changed_nodes.insert(id);
                changes.changed_layers.insert(id);
                changes.topology_changed = true;
                Ok(UndoMutation::NodeValue {
                    id,
                    kind: old_kind,
                    generation: old_generation,
                })
            }
            Mutation::Reparent { id, parent, before } => {
                if id == self.root {
                    return Err(RetainedSceneError::CannotRemoveRoot);
                }
                self.ensure_no_cycle(id, parent.node)?;
                let old_parent = self
                    .nodes
                    .get(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?
                    .parent
                    .expect("non-root node has parent");
                self.validate_child_insert(parent, before, id)?;
                let old_key = self.remove_child(old_parent, id)?;
                let new_insertion = self.insert_child(parent, before, id)?;
                record_rebalanced_siblings(&new_insertion, changes);
                self.nodes.get_mut(&id).expect("Mutation validation establishes node presence and parentage before applying changes").parent = Some(parent);
                changes.changed_nodes.insert(id);
                changes.topology_changed = true;
                changes.hierarchy_changed = true;
                Ok(UndoMutation::Reparent {
                    id,
                    old_parent,
                    old_key,
                    new_parent: parent,
                    new_insertion,
                })
            }
            Mutation::MoveBefore { id, sibling } => {
                let node = self
                    .nodes
                    .get(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?;
                if id == sibling {
                    return Ok(UndoMutation::None);
                }
                let parent = node.parent.ok_or(RetainedSceneError::CannotRemoveRoot)?;
                if self
                    .nodes
                    .get(&sibling)
                    .ok_or(RetainedSceneError::MissingNode(sibling))?
                    .parent
                    != Some(parent)
                {
                    return Err(RetainedSceneError::InvalidSibling(sibling));
                }
                let old_key = self.remove_child(parent, id)?;
                let new_insertion = self.insert_child(parent, Some(sibling), id)?;
                record_rebalanced_siblings(&new_insertion, changes);
                changes.changed_nodes.insert(id);
                changes.topology_changed = true;
                changes.hierarchy_changed = true;
                Ok(UndoMutation::Reparent {
                    id,
                    old_parent: parent,
                    old_key,
                    new_parent: parent,
                    new_insertion,
                })
            }
            Mutation::Remove { id } => {
                if id == self.root {
                    return Err(RetainedSceneError::CannotRemoveRoot);
                }
                let parent = self
                    .nodes
                    .get(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?
                    .parent
                    .expect("non-root node has parent");
                let parent_key = self.remove_child(parent, id)?;
                let mut removed = Vec::new();
                self.collect_subtree(id, &mut removed);
                let mut removed_nodes = Vec::with_capacity(removed.len());
                for removed_id in removed {
                    let node = self.nodes.remove(&removed_id).expect("Mutation validation establishes node presence and parentage before applying changes");
                    removed_nodes.push((removed_id, node));
                    changes.removed_nodes.insert(removed_id);
                }
                changes.topology_changed = true;
                changes.hierarchy_changed = true;
                Ok(UndoMutation::Remove {
                    parent,
                    parent_key,
                    nodes: removed_nodes,
                })
            }
            Mutation::Resize {
                width,
                height,
                scale,
            } => {
                validate_size(width, height, scale)?;
                if self.nodes.values().any(|node| match &node.kind {
                    NodeKind::Scene { canvas, .. } => {
                        (canvas.scale_factor() - scale).abs() > f32::EPSILON
                    }
                    _ => false,
                }) {
                    return Err(RetainedSceneError::ScaleMismatch);
                }
                if (self.width, self.height, self.scale.to_bits())
                    == (width, height, scale.to_bits())
                {
                    Ok(UndoMutation::None)
                } else {
                    let old = (self.width, self.height, self.scale);
                    self.width = width;
                    self.height = height;
                    self.scale = scale;
                    changes.surface_changed = true;
                    changes.invalidate_all = true;
                    Ok(UndoMutation::Resize(old))
                }
            }
            Mutation::InvalidateRect(rect) => {
                if ![rect.x0, rect.y0, rect.x1, rect.y1]
                    .into_iter()
                    .all(f64::is_finite)
                {
                    return Err(RetainedSceneError::InvalidPosition);
                }
                if !rect.is_zero_area() {
                    changes.invalidated_rects.push(rect);
                }
                Ok(UndoMutation::None)
            }
            Mutation::InvalidateAll => {
                changes.invalidate_all = true;
                Ok(UndoMutation::None)
            }
        }
    }
}

/// A sibling rebalance changes painter paths even though node values stay untouched.
/// Reporting every affected sibling keeps retained materializers from reusing stale order keys.
fn record_rebalanced_siblings(insertion: &ChildInsertion, changes: &mut SceneChangeSet) {
    if let Some(previous) = &insertion.before_rebalance {
        changes.changed_nodes.extend(previous.keys.keys().copied());
    }
}
