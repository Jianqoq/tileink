use super::helpers::chunk_layer_influence_bounds;
use super::*;
use crate::canvas::damage_history::ResolvedDamage;

impl PersistentSceneMaterializer {
    pub(super) fn layer_input_isolated(&self, id: RetainedNodeId) -> Option<bool> {
        let location = self.layer_command_locations.get(&id)?;
        let command = self
            .canvas
            .command_lists
            .get(location.parent_list)?
            .commands
            .get(location.command_index)?;
        match command {
            Command::Layer {
                layer, children, ..
            } => Some(
                !matches!(layer, Layer::Backdrop { .. }) && !self.canvas.can_fuse(layer, *children),
            ),
            Command::MaskLayer { .. } => Some(true),
            _ => None,
        }
    }

    pub(super) fn has_scoped_backdrop(&self, scene: &RetainedScene) -> bool {
        #[cfg(test)]
        self.scoped_backdrop_scans
            .set(self.scoped_backdrop_scans.get() + 1);
        self.nonlocal_dependencies.iter().any(|&id| {
            if self.chunks.get(&id).is_some_and(|chunk| {
                chunk
                    .backdrop_dependencies
                    .iter()
                    .any(|dependency| dependency.scoped)
            }) {
                return true;
            }
            let mut cursor = id;
            while let Some(parent) = scene.nodes.get(&cursor).and_then(|node| node.parent) {
                cursor = parent.node;
                if matches!(scene.nodes[&cursor].kind, NodeKind::Layer(_)) {
                    return true;
                }
            }
            false
        })
    }

    pub(super) fn local_output_bounds(
        &self,
        kind: &NodeKind,
        chunk: &SceneChunk,
    ) -> Option<Bounds> {
        #[cfg(test)]
        self.local_output_bounds_reads
            .set(self.local_output_bounds_reads.get() + 1);
        Some(match kind {
            NodeKind::Layer(_) => chunk_layer_influence_bounds(chunk),
            NodeKind::Scene { .. } => chunk.canvas.visual_bounds(),
            NodeKind::Group => return None,
        })
    }

    pub(super) fn publish_scoped_damage(
        &mut self,
        scene: &RetainedScene,
        previous: Option<&crate::canvas::RetainedFrame>,
        changes: &SceneChangeSet,
        local: Option<crate::canvas::RetainedDamage>,
        old_structure: Option<ResolvedDamage>,
    ) {
        let Some(previous) = previous else {
            return;
        };
        let scoped = self.scoped_backdrop;
        let resolved = if scoped {
            let local = old_structure
                .as_ref()
                .map(|_| self.new_structural_sources(scene, changes))
                .or(local);
            local.map(|mut local| {
                for &bounds in &self.canvas.invalidated_bounds {
                    local.add_unattributed(bounds);
                }
                let mut propagated = self.canvas.propagate_damage(&local);
                if let Some(old) = old_structure {
                    // Each hierarchy resolves its own painter order and input
                    // coordinates. Join only final root damage and cache IDs.
                    propagated.bounds.extend_from_slice(&old.bounds);
                    propagated
                        .dirty_backdrops
                        .extend(old.dirty_backdrops.iter().copied());
                }
                // Retain erased old root output alongside the current command tree's
                // dependency result. Deliver root bounds without propagating them again.
                let current = self.canvas.persistent_frame.as_ref().unwrap();
                for &id in &changes.changed_nodes {
                    propagated
                        .bounds
                        .extend(previous.node_state(id).map(|node| node.bounds));
                    propagated
                        .bounds
                        .extend(current.node_state(id).map(|node| node.bounds));
                }
                propagated.bounds.retain(|bounds| !bounds.is_empty());
                ResolvedDamage {
                    bounds: propagated.bounds.into_boxed_slice(),
                    dirty_backdrops: propagated
                        .dirty_backdrops
                        .into_iter()
                        .collect::<Vec<_>>()
                        .into(),
                }
            })
        } else {
            None
        };
        let history = previous.damage_history.advance(
            scoped,
            self.version.get(),
            scene.version.get(),
            resolved,
        );
        Rc::make_mut(&mut self.canvas)
            .persistent_frame
            .as_mut()
            .unwrap()
            .damage_history = history;
    }
}

#[cfg(test)]
mod tests;
