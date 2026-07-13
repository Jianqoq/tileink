use super::model::*;
use super::prelude::*;
use super::transaction::{Mutation, RetainedSceneTransaction, UndoMutation};
use super::validation::*;

pub(crate) const JOURNAL_CAPACITY: usize = 256;
static NEXT_SCENE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(crate) struct JournalEntry {
    version: SceneVersion,
    changes: SceneChangeSet,
}

pub struct RetainedScene {
    pub(crate) id: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale: f32,
    pub(crate) root: RetainedNodeId,
    pub(crate) version: SceneVersion,
    pub(crate) nodes: HashMap<RetainedNodeId, SceneNode>,
    journal: VecDeque<JournalEntry>,
    pub(crate) next_node_instance: u64,
}

impl RetainedScene {
    pub fn new(
        width: u32,
        height: u32,
        scale: f32,
        root: RetainedNodeId,
    ) -> Result<Self, RetainedSceneError> {
        validate_size(width, height, scale)?;
        let mut nodes = HashMap::default();
        nodes.insert(root, SceneNode::group(None, 1));
        Ok(Self {
            id: NEXT_SCENE_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            scale,
            root,
            version: SceneVersion::INITIAL,
            nodes,
            journal: VecDeque::new(),
            next_node_instance: 2,
        })
    }

    pub fn version(&self) -> SceneVersion {
        self.version
    }

    pub fn root(&self) -> RetainedNodeId {
        self.root
    }

    pub fn logical_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn physical_size(&self) -> (u32, u32) {
        (
            ((self.width as f64) * f64::from(self.scale)).ceil() as u32,
            ((self.height as f64) * f64::from(self.scale)).ceil() as u32,
        )
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub fn transaction(&mut self) -> RetainedSceneTransaction<'_> {
        RetainedSceneTransaction {
            scene: self,
            mutations: Vec::new(),
        }
    }

    pub(crate) fn changes_since(&self, version: SceneVersion) -> Option<SceneChangeSet> {
        if version == self.version {
            return Some(SceneChangeSet::default());
        }
        let first = self.journal.front()?.version.get();
        if version.get().saturating_add(1) < first {
            return None;
        }
        let mut changes = SceneChangeSet::default();
        for entry in self.journal.iter().filter(|entry| entry.version > version) {
            changes.merge(&entry.changes);
        }
        Some(changes)
    }

    #[cfg(test)]
    pub(crate) fn to_canvas(&self) -> Canvas {
        let mut canvas = Canvas::new(self.width, self.height, self.scale);
        self.append_children(&mut canvas, RetainedParent::content(self.root));
        canvas
    }

    #[cfg(test)]
    fn append_children(&self, canvas: &mut Canvas, parent: RetainedParent) {
        let node = &self.nodes[&parent.node];
        let children = node
            .children(parent.branch)
            .expect("validated scene branch");
        for child in children.values() {
            self.append_node(canvas, *child);
        }
    }

    #[cfg(test)]
    fn append_node(&self, canvas: &mut Canvas, id: RetainedNodeId) {
        let node = &self.nodes[&id];
        match &node.kind {
            NodeKind::Group => self.append_children(canvas, RetainedParent::content(id)),
            NodeKind::Scene {
                canvas: child,
                transform,
                ..
            } => {
                let mut transformed = Canvas::new(self.width, self.height, self.scale);
                transformed.append(child, Point::ZERO);
                transformed.set_retained_transform(*transform);
                canvas.append(&transformed, Point::ZERO);
            }
            NodeKind::Layer(layer) => {
                match layer {
                    RetainedLayerDescriptor::ClipPath {
                        path,
                        transform,
                        rule,
                        tolerance,
                    } => canvas.push_clip_layer(path.clone(), *transform, *rule, *tolerance),
                    RetainedLayerDescriptor::ClipSdf { sdf, transform } => {
                        assert!(canvas.push_clip_sdf_layer_transformed(*sdf, *transform));
                    }
                    RetainedLayerDescriptor::Isolate {
                        path,
                        transform,
                        tolerance,
                    } => canvas.push_isolate_layer(path.clone(), *transform, *tolerance),
                    RetainedLayerDescriptor::Opacity {
                        path,
                        transform,
                        tolerance,
                        opacity,
                    } => canvas.push_opacity_layer(path.clone(), *transform, *tolerance, *opacity),
                    RetainedLayerDescriptor::Blend {
                        path,
                        transform,
                        tolerance,
                        mix,
                        compose,
                    } => canvas.push_blend_layer(
                        path.clone(),
                        *transform,
                        *tolerance,
                        *mix,
                        *compose,
                    ),
                    RetainedLayerDescriptor::Filter {
                        filter,
                        sample_region,
                    } => canvas.push_filter_layer(filter.clone(), sample_region.clone()),
                    RetainedLayerDescriptor::Backdrop {
                        filter,
                        sample_region,
                    } => canvas.push_backdrop_layer(filter.clone(), sample_region.clone()),
                    RetainedLayerDescriptor::Mask(mask) => {
                        let mut mask_canvas = Canvas::new(self.width, self.height, self.scale);
                        self.append_children(&mut mask_canvas, RetainedParent::mask(id));
                        canvas.push_mask_layer(mask_canvas, mask.clone());
                    }
                }
                self.append_children(canvas, RetainedParent::content(id));
                canvas.pop_layer();
            }
        }
    }

    pub(crate) fn commit_mutations(
        &mut self,
        mutations: Vec<Mutation>,
    ) -> Result<SceneVersion, RetainedSceneError> {
        let mut changes = SceneChangeSet::default();
        let mut undo = Vec::with_capacity(mutations.len());
        for mutation in mutations {
            match self.apply_with_undo(mutation, &mut changes) {
                Ok(entry) => undo.push(entry),
                Err(error) => {
                    for entry in undo.into_iter().rev() {
                        self.undo(entry);
                    }
                    return Err(error);
                }
            }
        }
        if changes == SceneChangeSet::default() {
            return Ok(self.version);
        }
        self.version = SceneVersion(self.version.0.wrapping_add(1));
        self.journal.push_back(JournalEntry {
            version: self.version,
            changes,
        });
        while self.journal.len() > JOURNAL_CAPACITY {
            self.journal.pop_front();
        }
        Ok(self.version)
    }

    fn apply_with_undo(
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
                        canvas: current_canvas.clone(),
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
                self.nodes.get_mut(&id).unwrap().parent = Some(parent);
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
                if id == sibling {
                    return Ok(UndoMutation::None);
                }
                let parent = self
                    .nodes
                    .get(&id)
                    .ok_or(RetainedSceneError::MissingNode(id))?
                    .parent
                    .ok_or(RetainedSceneError::CannotRemoveRoot)?;
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
                    let node = self.nodes.remove(&removed_id).unwrap();
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
                    != (width, height, scale.to_bits())
                {
                    let old = (self.width, self.height, self.scale);
                    self.width = width;
                    self.height = height;
                    self.scale = scale;
                    changes.surface_changed = true;
                    changes.invalidate_all = true;
                    Ok(UndoMutation::Resize(old))
                } else {
                    Ok(UndoMutation::None)
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

    fn undo(&mut self, mutation: UndoMutation) {
        match mutation {
            UndoMutation::None => {}
            UndoMutation::Insert {
                parent,
                id,
                insertion,
            } => {
                self.undo_child_insertion(parent, id, insertion);
                self.nodes.remove(&id).expect("undo inserted node exists");
            }
            UndoMutation::NodeValue {
                id,
                kind,
                generation,
            } => {
                let node = self.nodes.get_mut(&id).expect("undo node exists");
                node.kind = kind;
                node.generation = generation;
            }
            UndoMutation::Reparent {
                id,
                old_parent,
                old_key,
                new_parent,
                new_insertion,
            } => {
                self.undo_child_insertion(new_parent, id, new_insertion);
                self.insert_child_at(old_parent, old_key, id);
                self.nodes.get_mut(&id).unwrap().parent = Some(old_parent);
            }
            UndoMutation::Remove {
                parent,
                parent_key,
                nodes,
            } => {
                let root = nodes[0].0;
                self.nodes.extend(nodes);
                self.insert_child_at(parent, parent_key, root);
            }
            UndoMutation::Resize((width, height, scale)) => {
                (self.width, self.height, self.scale) = (width, height, scale);
            }
        }
    }

    fn insert_child(
        &mut self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
    ) -> Result<ChildInsertion, RetainedSceneError> {
        self.nodes
            .get_mut(&parent.node)
            .ok_or(RetainedSceneError::MissingNode(parent.node))?
            .children_mut(parent.node, parent.branch)?
            .insert_before(id, before)
    }

    fn undo_child_insertion(
        &mut self,
        parent: RetainedParent,
        id: RetainedNodeId,
        insertion: ChildInsertion,
    ) {
        if let Some(before_rebalance) = insertion.before_rebalance {
            *self
                .nodes
                .get_mut(&parent.node)
                .expect("undo parent exists")
                .children_mut(parent.node, parent.branch)
                .expect("undo branch is valid") = before_rebalance;
        } else {
            let removed = self.remove_child(parent, id);
            debug_assert_eq!(removed, Ok(insertion.key));
        }
    }

    fn validate_child_insert(
        &self,
        parent: RetainedParent,
        before: Option<RetainedNodeId>,
        id: RetainedNodeId,
    ) -> Result<(), RetainedSceneError> {
        let children = self
            .nodes
            .get(&parent.node)
            .ok_or(RetainedSceneError::MissingNode(parent.node))?
            .children(parent.branch)?;
        if let Some(before) = before
            && (before == id || children.key_of(before).is_none())
        {
            return Err(RetainedSceneError::InvalidSibling(before));
        }
        Ok(())
    }

    fn insert_child_at(&mut self, parent: RetainedParent, key: u128, id: RetainedNodeId) {
        self.nodes
            .get_mut(&parent.node)
            .expect("undo parent exists")
            .children_mut(parent.node, parent.branch)
            .expect("undo branch is valid")
            .insert_at(key, id);
    }

    fn remove_child(
        &mut self,
        parent: RetainedParent,
        id: RetainedNodeId,
    ) -> Result<u128, RetainedSceneError> {
        self.nodes
            .get_mut(&parent.node)
            .ok_or(RetainedSceneError::MissingNode(parent.node))?
            .children_mut(parent.node, parent.branch)?
            .remove(id)
            .ok_or(RetainedSceneError::MissingNode(id))
    }

    fn ensure_no_cycle(
        &self,
        id: RetainedNodeId,
        mut parent: RetainedNodeId,
    ) -> Result<(), RetainedSceneError> {
        loop {
            if parent == id {
                return Err(RetainedSceneError::Cycle(id));
            }
            let node = self
                .nodes
                .get(&parent)
                .ok_or(RetainedSceneError::MissingNode(parent))?;
            let Some(next) = node.parent else {
                return Ok(());
            };
            parent = next.node;
        }
    }

    pub(crate) fn collect_subtree(&self, id: RetainedNodeId, out: &mut Vec<RetainedNodeId>) {
        out.push(id);
        let node = &self.nodes[&id];
        for child in node.content.values().chain(node.mask.values()) {
            self.collect_subtree(*child, out);
        }
    }
}
