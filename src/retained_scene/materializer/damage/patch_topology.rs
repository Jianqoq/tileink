use super::*;

impl PersistentSceneMaterializer {
    pub(crate) fn patch_topology_frame_override(
        &mut self,
        scene: &RetainedScene,
        previous: Option<crate::canvas::RetainedFrame>,
        changes: &SceneChangeSet,
        damage: &[(RetainedNodeId, Bounds)],
    ) -> bool {
        let Some(mut frame) = previous else {
            return false;
        };
        let mut patches = Vec::new();
        for &id in &changes.removed_nodes {
            if let Some(old) = frame.node_state(id) {
                patches.push(RetainedNodePatch {
                    old: Some(old),
                    new: None,
                    damage: None,
                });
            }
        }
        for &id in &changes.changed_nodes {
            let Some(node) = scene.nodes.get(&id) else {
                continue;
            };
            let old = frame.node_state(id);
            // A reorder can change structural groups, which never own draw chunks.
            // Resolve chunks only for drawable node kinds; a missing drawable stays an invariant error.
            let (bounds, kind) = match &node.kind {
                NodeKind::Scene { .. } => {
                    let chunk = &self.chunks[&id];
                    let natural = self.influenced_bounds(scene, id, chunk.canvas.visual_bounds());
                    (
                        self.retained_output_bounds(scene, id, natural),
                        RetainedNodeKind::Scene,
                    )
                }
                NodeKind::Layer(_) => {
                    let chunk = &self.chunks[&id];
                    let mut bounds =
                        self.influenced_bounds(scene, id, chunk_layer_influence_bounds(chunk));
                    let mut leaves = Vec::new();
                    collect_scene_leaves(scene, id, &mut leaves);
                    for leaf_id in leaves {
                        let leaf = &self.chunks[&leaf_id];
                        let natural =
                            self.influenced_bounds(scene, leaf_id, leaf.canvas.visual_bounds());
                        bounds = bounds.union(self.retained_output_bounds(scene, leaf_id, natural));
                    }
                    (bounds, RetainedNodeKind::Layer)
                }
                NodeKind::Group => continue,
            };
            patches.push(RetainedNodePatch {
                old,
                new: Some(crate::canvas::RetainedNodeState {
                    id,
                    revision: NodeRevision::new(node.instance, node.generation),
                    bounds,
                    kind,
                }),
                damage: self.fixed_translation_bounds(scene, id),
            });
        }
        let mut explicit_damage = damage.to_vec();
        explicit_damage.extend(patches.iter().map(|patch| {
            let node = patch.new.or(patch.old).unwrap();
            let bounds = patch
                .damage_bounds()
                .expect("retained patch has at least one state or explicit damage");
            (node.id, bounds)
        }));
        let index = retained_patch_index(&patches, frame.delta.as_ref().map(|delta| &delta.index));
        let (previous, depth) = prune_shadowed_delta(frame.delta.clone(), &index);
        if depth > 255 {
            return false;
        }
        for patch in &patches {
            let id = patch.new.or(patch.old).unwrap().id;
            if patch.old.map(|node| node.bounds) != patch.new.map(|node| node.bounds) {
                self.set_node_bounds_spatial(
                    id,
                    patch.new.map(|node| node.bounds),
                    self.fixed_translation_bounds(scene, id),
                );
            }
            if let Some(chunk) = self.chunks.get(&id) {
                let raw_bounds = match scene.nodes[&id].kind {
                    NodeKind::Layer(_) => chunk_layer_influence_bounds(chunk),
                    NodeKind::Scene { .. } => chunk.canvas.visual_bounds(),
                    NodeKind::Group => unreachable!("groups do not own chunks"),
                };
                self.set_raw_node_bounds_spatial(
                    id,
                    Some(raw_bounds),
                    self.fixed_translation_bounds(scene, id),
                );
            } else {
                self.set_raw_node_bounds_spatial(id, None, None);
            }
        }
        frame.version = scene.version.get();
        frame.delta = Some(Rc::new(RetainedFrameDelta {
            from_version: self.version.get(),
            to_version: scene.version.get(),
            patches: reuse_empty_slice(patches, frame.delta.as_ref().map(|delta| &delta.patches)),
            previous,
            depth,
            damage: reuse_empty_slice(
                explicit_damage,
                frame.delta.as_ref().map(|delta| &delta.damage),
            ),
            dirty_backdrops: reuse_empty_slice(
                Vec::new(),
                frame.delta.as_ref().map(|delta| &delta.dirty_backdrops),
            ),
            backdrop_damage_complete: false,
            index,
        }));
        let canvas = Rc::make_mut(&mut self.canvas);
        frame.invalidated_bounds = canvas.invalidated_bounds.clone();
        frame.invalidate_all = canvas.invalidate_all;
        frame.requires_damage_propagation = !self.nonlocal_dependencies.is_empty();
        canvas.persistent_frame = Some(frame);
        true
    }
}
