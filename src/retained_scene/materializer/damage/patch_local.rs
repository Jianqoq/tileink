use super::*;

impl PersistentSceneMaterializer {
    /// Patches frame metadata for local clip-like layer geometry changes without walking every
    /// descendant. Raw bounds are indexed independently from currently clipped bounds so an
    /// expanding clip can find nodes that were completely invisible in the previous frame.
    #[allow(
        clippy::expect_used,
        reason = "Local layer patches are constructed with both old and new node states"
    )]
    pub(crate) fn patch_local_layer_frame_override(
        &mut self,
        scene: &RetainedScene,
        previous: Option<crate::canvas::RetainedFrame>,
        layer_bounds: &[(RetainedNodeId, Bounds, Bounds)],
    ) -> bool {
        let Some(mut frame) = previous else {
            return false;
        };
        if layer_bounds.is_empty()
            || layer_bounds
                .iter()
                .any(|&(id, _, _)| !Self::has_local_bounds_ancestry(scene, id))
        {
            return false;
        }

        let mut candidates = layer_bounds
            .iter()
            .map(|&(id, _, _)| id)
            .collect::<HashSet<_>>();
        for &(_, old, new) in layer_bounds {
            candidates.extend(self.raw_spatial_candidates(old.union(new)));
        }
        candidates.retain(|&id| {
            layer_bounds
                .iter()
                .any(|&(layer, _, _)| is_descendant_or_self(scene, id, layer))
        });

        let mut patches = Vec::with_capacity(candidates.len());
        for id in candidates {
            let Some(old) = frame.node_state(id) else {
                return false;
            };
            let node = &scene.nodes[&id];
            let (raw_bounds, bounds, kind) = match &node.kind {
                NodeKind::Scene { .. } => {
                    let raw = self.chunks[&id].canvas.visual_bounds();
                    (
                        raw,
                        self.retained_output_bounds(
                            scene,
                            id,
                            self.influenced_bounds(scene, id, raw),
                        ),
                        RetainedNodeKind::Scene,
                    )
                }
                NodeKind::Layer(_) => {
                    if !Self::has_local_bounds_ancestry(scene, id) {
                        return false;
                    }
                    let raw = chunk_layer_influence_bounds(&self.chunks[&id]);
                    (
                        raw,
                        self.influenced_bounds(scene, id, raw),
                        RetainedNodeKind::Layer,
                    )
                }
                NodeKind::Group => continue,
            };
            let new = crate::canvas::RetainedNodeState {
                id,
                revision: NodeRevision::new(node.instance, node.generation),
                bounds,
                kind,
            };
            self.set_raw_node_bounds_spatial(
                id,
                Some(raw_bounds),
                self.fixed_translation_bounds(scene, id),
            );
            if old != new {
                patches.push(RetainedNodePatch {
                    old: Some(old),
                    new: Some(new),
                    damage: self.fixed_translation_bounds(scene, id),
                });
            }
        }
        if patches.is_empty() {
            return false;
        }

        let index = retained_patch_index(&patches, frame.delta.as_ref().map(|delta| &delta.index));
        let (previous, depth) = prune_shadowed_delta(frame.delta.clone(), &index);
        let source_damage = patches
            .iter()
            .map(|patch| {
                let old = patch.old.expect(
                    "Local layer patches are constructed with both old and new node states",
                );
                let new = patch.new.expect(
                    "Local layer patches are constructed with both old and new node states",
                );
                (Some(new.id), old.bounds.union(new.bounds))
            })
            .chain(
                self.canvas
                    .invalidated_bounds
                    .iter()
                    .copied()
                    .map(|bounds| (None, bounds)),
            )
            .collect::<Vec<_>>();
        let (backdrop_damage, dirty_backdrops) =
            if self.nonlocal_dependencies.is_empty() || self.scoped_backdrop {
                (Vec::new(), Vec::new())
            } else {
                self.incremental_backdrop_damage(scene, &source_damage)
            };
        for patch in &patches {
            let new = patch
                .new
                .expect("Local layer patches are constructed with both old and new node states");
            if patch
                .old
                .expect("Local layer patches are constructed with both old and new node states")
                .bounds
                != new.bounds
            {
                self.set_node_bounds_spatial(
                    new.id,
                    Some(new.bounds),
                    self.fixed_translation_bounds(scene, new.id),
                );
            }
        }

        frame.version = scene.version.get();
        let mut delta = RetainedFrameDelta {
            from_version: self.version.get(),
            to_version: scene.version.get(),
            patches: reuse_empty_slice(patches, frame.delta.as_ref().map(|delta| &delta.patches)),
            previous,
            depth,
            damage: reuse_empty_slice(
                backdrop_damage,
                frame.delta.as_ref().map(|delta| &delta.damage),
            ),
            dirty_backdrops: reuse_empty_slice(
                dirty_backdrops,
                frame.delta.as_ref().map(|delta| &delta.dirty_backdrops),
            ),
            backdrop_damage_complete: !self.scoped_backdrop,
            index,
        };
        if depth > 255 {
            if !Self::compact_content_state_pages(&mut frame, &delta) {
                return false;
            }
            delta.previous = None;
            delta.depth = 1;
        }
        frame.delta = Some(Rc::new(delta));
        let canvas = Rc::make_mut(&mut self.canvas);
        frame
            .invalidated_bounds
            .clone_from(&canvas.invalidated_bounds);
        frame.invalidate_all = canvas.invalidate_all;
        frame.requires_damage_propagation = !self.nonlocal_dependencies.is_empty();
        canvas.persistent_frame = Some(frame);
        true
    }

    pub(crate) fn has_local_bounds_ancestry(scene: &RetainedScene, mut id: RetainedNodeId) -> bool {
        if let NodeKind::Layer(layer) = &scene.nodes[&id].kind
            && !is_local_bounds_layer(layer)
        {
            return false;
        }
        while let Some(parent) = scene.nodes[&id].parent {
            id = parent.node;
            if let NodeKind::Layer(layer) = &scene.nodes[&id].kind
                && !is_local_bounds_layer(layer)
            {
                return false;
            }
        }
        true
    }
}
