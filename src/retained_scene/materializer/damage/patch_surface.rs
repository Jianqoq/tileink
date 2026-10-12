use super::*;

impl PersistentSceneMaterializer {
    pub(crate) fn patch_surface_frame(
        &mut self,
        scene: &RetainedScene,
        previous: Option<crate::canvas::RetainedFrame>,
        changed: &HashSet<RetainedNodeId>,
    ) -> bool {
        let Some(mut frame) = previous else {
            return false;
        };
        // Surface redraws do not need node damage, but retained filter caches created by that
        // redraw are keyed by node revision. Patch changed nodes before publishing the resized
        // frame so the next incremental edit can reuse its pre-backdrop source history instead of
        // sampling already-filtered clean pixels from the root target.
        let patches = changed
            .iter()
            .filter(|id| {
                self.layer_nodes.contains(id)
                    || self
                        .chunks
                        .get(id)
                        .is_some_and(|chunk| !chunk.plain_fragment)
            })
            .filter_map(|&id| {
                let old = frame.node_state(id)?;
                let node = &scene.nodes[&id];
                let new = crate::canvas::RetainedNodeState {
                    revision: NodeRevision::new(node.instance, node.generation),
                    ..old
                };
                (old != new).then_some(RetainedNodePatch {
                    old: Some(old),
                    new: Some(new),
                    damage: None,
                })
            })
            .collect::<Vec<_>>();
        if !patches.is_empty() {
            let index =
                retained_patch_index(&patches, frame.delta.as_ref().map(|delta| &delta.index));
            let (previous, depth) = prune_shadowed_delta(frame.delta.clone(), &index);
            let mut delta = RetainedFrameDelta {
                from_version: self.version.get(),
                to_version: scene.version.get(),
                patches: reuse_empty_slice(
                    patches,
                    frame.delta.as_ref().map(|delta| &delta.patches),
                ),
                previous,
                depth,
                damage: reuse_empty_slice(
                    Vec::new(),
                    frame.delta.as_ref().map(|delta| &delta.damage),
                ),
                dirty_backdrops: reuse_empty_slice(
                    Vec::new(),
                    frame.delta.as_ref().map(|delta| &delta.dirty_backdrops),
                ),
                backdrop_damage_complete: false,
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
        }
        let physical_size = self.canvas.physical_size();
        frame.logical_size = (scene.width, scene.height);
        frame.physical_size = physical_size;
        frame.scale_bits = scene.scale.to_bits();
        frame.version = scene.version.get();
        let canvas = Rc::make_mut(&mut self.canvas);
        frame.invalidated_bounds = canvas.invalidated_bounds.clone();
        frame.invalidate_all = true;
        canvas.persistent_frame = Some(frame);
        self.surface_metadata_stale = true;
        true
    }

    /// Turns a lazily rebuilt post-resize frame into the actual pre-mutation baseline.
    ///
    /// At this point `scene` already exposes the incoming content generations, but the materialized
    /// chunks still contain the pixels rendered by the resize frame. Keeping the incoming revisions
    /// would make the first later edit compare equal and incorrectly produce zero damage.
    pub(crate) fn restore_deferred_surface_baseline(&mut self, changed: &HashSet<RetainedNodeId>) {
        let Some(frame) = Rc::make_mut(&mut self.canvas).persistent_frame.as_mut() else {
            return;
        };
        frame.version = self.version.get();
        let nodes = Rc::make_mut(&mut frame.nodes);
        for id in changed {
            let Some(metadata) = self.node_metadata.get(id) else {
                continue;
            };
            let Some(&index) = frame.node_index.get(id) else {
                continue;
            };
            nodes[index].revision = NodeRevision::new(metadata.instance, metadata.generation);
        }
    }
}
