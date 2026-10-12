use super::*;

impl PersistentSceneMaterializer {
    pub(crate) fn patch_frame_override(
        &mut self,
        scene: &RetainedScene,
        previous: Option<crate::canvas::RetainedFrame>,
        changed: &HashSet<RetainedNodeId>,
    ) {
        let Some(mut frame) = previous else {
            self.rebuild_frame_override(scene);
            return;
        };
        let mut patches = Vec::with_capacity(changed.len());
        for &id in changed {
            let Some(old) = frame.node_state(id) else {
                self.rebuild_frame_override(scene);
                return;
            };
            let node = &scene.nodes[&id];
            let Some(chunk) = self.chunks.get(&id) else {
                self.rebuild_frame_override(scene);
                return;
            };
            let fixed_bounds = self.fixed_translation_bounds_for_node(scene, node);
            // A bounded translation already supplies the conservative output and spatial domain.
            // Keep the last exact raw bound cached: walking every command in every translated
            // child only to discard the result in `retained_output_bounds` made chart p95 scale
            // with the number of cached candles.
            let (raw_bounds, output_bounds) = if let Some(fixed) = fixed_bounds {
                (
                    self.raw_node_bounds.get(&id).copied().unwrap_or(fixed),
                    fixed,
                )
            } else {
                // Layer chunks encode only the layer shell. Its empty child list is not
                // the layer's output domain; using visual_bounds erased opacity/clip
                // output on parameter updates. Keep the same domain as frame construction.
                let raw = match node.kind {
                    NodeKind::Layer(_) => chunk_layer_influence_bounds(chunk),
                    _ => chunk.canvas.visual_bounds(),
                };
                (
                    raw,
                    self.retained_output_bounds(scene, id, self.influenced_bounds(scene, id, raw)),
                )
            };
            let new = crate::canvas::RetainedNodeState {
                revision: NodeRevision::new(node.instance, node.generation),
                // Position-only patches in scenes with layers still need the new influenced
                // bounds. Keeping `old.bounds` forced later frames to rebuild the full retained
                // frame/spatial index and missed the moved node's new backdrop dependencies.
                bounds: output_bounds,
                kind: match node.kind {
                    NodeKind::Layer(_) => RetainedNodeKind::Layer,
                    NodeKind::Scene { .. } => RetainedNodeKind::Scene,
                    NodeKind::Group => {
                        self.rebuild_frame_override(scene);
                        return;
                    }
                },
                ..old
            };
            patches.push(RetainedNodePatch {
                old: Some(old),
                new: Some(new),
                damage: fixed_bounds,
            });
            self.set_raw_node_bounds_spatial(id, Some(raw_bounds), fixed_bounds);
        }
        let index = retained_patch_index(&patches, frame.delta.as_ref().map(|delta| &delta.index));
        let (previous, depth) = prune_shadowed_delta(frame.delta.clone(), &index);
        let (backdrop_damage, dirty_backdrops) =
            if self.nonlocal_dependencies.is_empty() || self.scoped_backdrop {
                (Vec::new(), Vec::new())
            } else {
                let source_damage = patches
                    .iter()
                    .map(|patch| {
                        let node = patch.new.or(patch.old).unwrap();
                        let bounds = patch
                            .damage_bounds()
                            .expect("retained patch has a node or explicit damage");
                        (Some(node.id), bounds)
                    })
                    .chain(
                        self.canvas
                            .invalidated_bounds
                            .iter()
                            .copied()
                            .map(|bounds| (None, bounds)),
                    )
                    .collect::<Vec<_>>();
                self.incremental_backdrop_damage(scene, &source_damage)
            };
        for patch in &patches {
            if patch.old.map(|node| node.bounds) != patch.new.map(|node| node.bounds) {
                let id = patch.new.unwrap().id;
                // The patch already carries this node's fixed translation domain.
                self.set_node_bounds_spatial(id, patch.new.map(|node| node.bounds), patch.damage);
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
                self.rebuild_frame_override(scene);
                self.rebuild_spatial_index(scene);
                return;
            }
            delta.previous = None;
            delta.depth = 1;
        }
        frame.delta = Some(Rc::new(delta));
        frame.invalidated_bounds = Rc::make_mut(&mut self.canvas).invalidated_bounds.clone();
        frame.invalidate_all = Rc::make_mut(&mut self.canvas).invalidate_all;
        frame.requires_damage_propagation = !self.nonlocal_dependencies.is_empty();
        Rc::make_mut(&mut self.canvas).persistent_frame = Some(frame);
    }

    pub(crate) fn compact_content_state_pages(
        frame: &mut crate::canvas::RetainedFrame,
        delta: &RetainedFrameDelta,
    ) -> bool {
        const PAGE_SIZE: usize = 256;
        let mut resolved =
            HashMap::<RetainedNodeId, (usize, crate::canvas::RetainedNodeState)>::default();
        let mut current = Some(delta);
        while let Some(delta) = current {
            for patch in delta.patches.iter() {
                let Some(node) = patch.new else {
                    return false;
                };
                if resolved.contains_key(&node.id) {
                    continue;
                }
                let Some(&index) = frame.node_index.get(&node.id) else {
                    return false;
                };
                resolved.insert(node.id, (index, node));
            }
            current = delta.previous.as_deref();
        }
        let mut updates =
            HashMap::<usize, Vec<(usize, crate::canvas::RetainedNodeState)>>::default();
        for (_, (index, node)) in resolved {
            updates
                .entry(index / PAGE_SIZE)
                .or_default()
                .push((index % PAGE_SIZE, node));
        }
        let base = frame.nodes.clone();
        let pages = Rc::make_mut(&mut frame.state_pages);
        for (page_index, updates) in updates {
            let start = page_index * PAGE_SIZE;
            let end = (start + PAGE_SIZE).min(base.len());
            let mut page = pages
                .get(&page_index)
                .map(|page| page.to_vec())
                .unwrap_or_else(|| base[start..end].to_vec());
            for (offset, node) in updates {
                page[offset] = node;
            }
            pages.insert(page_index, page.into());
        }
        true
    }
}
