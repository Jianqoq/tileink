use super::helpers::chunk_layer_influence_bounds;
use super::*;
use crate::canvas::damage_history::ResolvedDamage;
use crate::shared::execution::ROOT_COMMAND_LIST_ID;

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
                !matches!(layer, Layer::Backdrop { opacity: None, .. })
                    && !self.canvas.can_fuse(layer, *children),
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
        id: RetainedNodeId,
        kind: &NodeKind,
        chunk: &SceneChunk,
    ) -> Option<Bounds> {
        #[cfg(test)]
        self.local_output_bounds_reads
            .set(self.local_output_bounds_reads.get() + 1);
        Some(match kind {
            NodeKind::Layer(_) => self.layer_output_bounds(id, chunk),
            NodeKind::Scene { .. } => chunk.canvas.visual_bounds(),
            NodeKind::Group => return None,
        })
    }

    #[allow(
        clippy::expect_used,
        reason = "Scoped damage is published into the persistent frame installed by materialization"
    )]
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
                let current = self.canvas.persistent_frame.as_ref().expect("Scoped damage is published into the persistent frame installed by materialization");
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
            .expect(
                "Scoped damage is published into the persistent frame installed by materialization",
            )
            .damage_history = history;
    }

    /// A layer shell has no child commands. Unbounded opacity gets its output
    /// domain from the installed subtree; treating the empty shell as its output
    /// erased frame damage after the first alpha update and left stale pixels.
    pub(super) fn layer_output_bounds(&self, id: RetainedNodeId, chunk: &SceneChunk) -> Bounds {
        if matches!(
            &chunk.canvas.command_lists[ROOT_COMMAND_LIST_ID].commands[0],
            Command::Layer {
                layer: Layer::BlurGroup { .. } | Layer::Opacity { draw: None, .. },
                ..
            }
        ) {
            let location = self.layer_command_locations[&id];
            let Command::Layer {
                layer, children, ..
            } = &self.canvas.command_lists[location.parent_list].commands[location.command_index]
            else {
                unreachable!("a retained opacity layer owns a layer command");
            };
            let bounds = self.canvas.command_list_visual_bounds(*children);
            if bounds.is_empty() {
                return bounds;
            }
            match layer {
                Layer::BlurGroup { sigma } => filter::unclipped_filtered_region_bounds(
                    &filter::Filter::Blur {
                        std_dev_x: *sigma,
                        std_dev_y: *sigma,
                        sampling: crate::BlurSampling::default(),
                    },
                    &self.canvas.paint_region(*children),
                ),
                _ => bounds,
            }
        } else {
            chunk_layer_influence_bounds(chunk)
        }
    }
}

#[cfg(test)]
mod tests;
