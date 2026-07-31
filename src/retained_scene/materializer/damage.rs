use super::helpers::*;
use super::*;

impl PersistentSceneMaterializer {
    pub(crate) fn influenced_bounds(
        &self,
        scene: &RetainedScene,
        mut id: RetainedNodeId,
        mut bounds: Bounds,
    ) -> Bounds {
        let canvas_bounds =
            Bounds::canvas(self.canvas.physical_width(), self.canvas.physical_height());
        while let Some(parent) = scene.nodes[&id].parent {
            let parent_node = &scene.nodes[&parent.node];
            if let NodeKind::Layer(layer) = &parent_node.kind {
                bounds = match layer {
                    RetainedLayerDescriptor::ClipPath { .. }
                    | RetainedLayerDescriptor::ClipSdf { .. }
                    | RetainedLayerDescriptor::Isolate { .. }
                    | RetainedLayerDescriptor::Opacity { .. }
                    | RetainedLayerDescriptor::Blend { .. } => self.chunks[&parent.node]
                        .canvas
                        .draw_records
                        .first()
                        .map_or(bounds, |draw| {
                            let clip = draw.pixel_bounds;
                            bounds.intersect(Bounds::new(clip.x0, clip.y0, clip.x1, clip.y1))
                        }),
                    RetainedLayerDescriptor::Filter {
                        filter: value,
                        sample_region,
                    } => {
                        let dependency = filter::region_bounds(sample_region)
                            .outset(filter::filter_dependency_outset(value));
                        let changed = bounds.intersect(dependency);
                        if changed.is_empty() {
                            Bounds::new(0, 0, 0, 0)
                        } else {
                            changed
                                .outset(filter::filter_outset(value))
                                .intersect(filter::unclipped_filtered_region_bounds(
                                    value,
                                    sample_region,
                                ))
                                .intersect(canvas_bounds)
                        }
                    }
                    RetainedLayerDescriptor::Mask(mask) => {
                        bounds.intersect(filter::region_bounds(&mask.region))
                    }
                    RetainedLayerDescriptor::Backdrop { .. } => bounds,
                };
            }
            id = parent.node;
        }
        bounds
    }

    pub(crate) fn fixed_translation_bounds(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> Option<Bounds> {
        let NodeKind::Scene {
            translation_damage: Some(rect),
            ..
        } = &scene.nodes.get(&id)?.kind
        else {
            return None;
        };
        let scale = f64::from(scene.scale);
        Some(
            Bounds::new(
                (rect.x0 * scale).floor() as i32,
                (rect.y0 * scale).floor() as i32,
                (rect.x1 * scale).ceil() as i32,
                (rect.y1 * scale).ceil() as i32,
            )
            .intersect(Bounds::canvas(
                self.canvas.physical_width(),
                self.canvas.physical_height(),
            )),
        )
    }

    pub(crate) fn retained_output_bounds(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        natural: Bounds,
    ) -> Bounds {
        self.fixed_translation_bounds(scene, id).unwrap_or(natural)
    }

    /// Resolves backdrop dependencies from the persistent hierarchy and painter index. Frame
    /// node bounds already include ordinary filter influence, while a backdrop additionally
    /// depends on earlier siblings intersecting its sample region.
    pub(crate) fn incremental_backdrop_damage(
        &self,
        scene: &RetainedScene,
        sources: &[(Option<RetainedNodeId>, Bounds)],
    ) -> (Vec<(RetainedNodeId, Bounds)>, Vec<RetainedNodeId>) {
        let mut damage = HashMap::<RetainedNodeId, Bounds>::default();
        let mut dirty = Vec::new();
        let mut source_nodes = HashSet::default();
        let mut ordered_sources = Vec::new();
        let mut damage_tiles = HashMap::default();
        for &(source, bounds) in sources {
            let Some(source) = source else {
                self.add_indexed_damage(&mut damage_tiles, bounds);
                continue;
            };
            source_nodes.insert(source);
            let Some(painter) = self.painter_bases.get(&source).cloned().or_else(|| {
                scene
                    .nodes
                    .contains_key(&source)
                    .then(|| painter_path(scene, source))
            }) else {
                continue;
            };
            ordered_sources.push((painter, bounds));
        }
        ordered_sources.sort_unstable_by(|left, right| left.0.as_ref().cmp(right.0.as_ref()));
        let mut source_cursor = 0;
        let mut backdrops = self
            .nonlocal_dependencies
            .iter()
            .copied()
            .map(|id| {
                let painter = self
                    .painter_bases
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| painter_path(scene, id));
                (id, painter)
            })
            .collect::<Vec<_>>();
        backdrops.sort_unstable_by(|left, right| left.1.as_ref().cmp(right.1.as_ref()));
        for (backdrop, backdrop_painter) in backdrops {
            let Some(chunk) = self.chunks.get(&backdrop) else {
                continue;
            };
            while source_cursor < ordered_sources.len()
                && ordered_sources[source_cursor].0.as_ref() < backdrop_painter.as_ref()
            {
                self.add_indexed_damage(&mut damage_tiles, ordered_sources[source_cursor].1);
                source_cursor += 1;
            }
            let mut affected_output = if source_nodes.contains(&backdrop) {
                self.node_bounds.get(&backdrop).copied().unwrap_or_else(|| {
                    chunk
                        .backdrop_dependencies
                        .iter()
                        .fold(Bounds::new(0, 0, 0, 0), |bounds, dependency| {
                            bounds.union(dependency.output)
                        })
                })
            } else {
                Bounds::new(0, 0, 0, 0)
            };
            for dependency in &chunk.backdrop_dependencies {
                let sampled =
                    self.indexed_damage_intersection(&damage_tiles, dependency.dependency);
                if !sampled.is_empty() {
                    affected_output = affected_output.union(
                        sampled
                            .outset(dependency.output_outset)
                            .intersect(dependency.output),
                    );
                }
            }
            if !affected_output.is_empty() {
                dirty.push(backdrop);
                // A changed earlier backdrop becomes sampled background for later backdrops. Tile
                // unions keep cascade propagation proportional to affected pixels instead of the
                // number of all earlier backdrop/source pairs.
                self.add_indexed_damage(&mut damage_tiles, affected_output);
                damage
                    .entry(backdrop)
                    .and_modify(|current| *current = current.union(affected_output))
                    .or_insert(affected_output);
            }
        }
        (damage.into_iter().collect(), dirty)
    }

    pub(crate) fn add_indexed_damage(&self, tiles: &mut HashMap<usize, Bounds>, bounds: Bounds) {
        for tile in self.tiles_for_bounds(bounds) {
            tiles
                .entry(tile)
                .and_modify(|current| *current = current.union(bounds))
                .or_insert(bounds);
        }
    }

    pub(crate) fn indexed_damage_intersection(
        &self,
        tiles: &HashMap<usize, Bounds>,
        bounds: Bounds,
    ) -> Bounds {
        self.tiles_for_bounds(bounds)
            .filter_map(|tile| tiles.get(&tile).copied())
            .map(|damage| damage.intersect(bounds))
            .fold(Bounds::new(0, 0, 0, 0), Bounds::union)
    }

    /// Patches frame metadata for local clip-like layer geometry changes without walking every
    /// descendant. Raw bounds are indexed independently from currently clipped bounds so an
    /// expanding clip can find nodes that were completely invisible in the previous frame.
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
                revision: NodeGeneration::new(node.generation),
                bounds,
                kind,
                placement_bits: None,
                ..old
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

        let index = retained_patch_index(&patches);
        let (previous, depth) = prune_shadowed_delta(frame.delta.clone(), &index);
        let source_damage = patches
            .iter()
            .map(|patch| {
                let old = patch.old.unwrap();
                let new = patch.new.unwrap();
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
        let (backdrop_damage, dirty_backdrops) = if self.nonlocal_dependencies.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            self.incremental_backdrop_damage(scene, &source_damage)
        };
        for patch in &patches {
            let new = patch.new.unwrap();
            if patch.old.unwrap().bounds != new.bounds {
                self.set_node_bounds_spatial(
                    new.id,
                    Some(new.bounds),
                    self.fixed_translation_bounds(scene, new.id),
                );
            }
        }

        frame.version = Some(scene.version.get());
        let mut delta = RetainedFrameDelta {
            from_version: self.version.get(),
            to_version: scene.version.get(),
            patches: patches.into(),
            previous,
            depth,
            damage: backdrop_damage.into(),
            dirty_backdrops: dirty_backdrops.into(),
            backdrop_damage_complete: true,
            index: Rc::new(index),
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
        frame.invalidated_bounds = canvas.invalidated_bounds.clone();
        frame.invalidate_all = canvas.invalidate_all;
        frame.dependency_free = self.dependency_free;
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
            let fixed_bounds = self.fixed_translation_bounds(scene, id);
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
                let raw = chunk.canvas.visual_bounds();
                (
                    raw,
                    self.retained_output_bounds(scene, id, self.influenced_bounds(scene, id, raw)),
                )
            };
            let new = crate::canvas::RetainedNodeState {
                revision: NodeGeneration::new(node.generation),
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
                placement_bits: None,
                ..old
            };
            patches.push(RetainedNodePatch {
                old: Some(old),
                new: Some(new),
                damage: fixed_bounds,
            });
            self.set_raw_node_bounds_spatial(id, Some(raw_bounds), fixed_bounds);
        }
        let index = retained_patch_index(&patches);
        let (previous, depth) = prune_shadowed_delta(frame.delta.clone(), &index);
        let (backdrop_damage, dirty_backdrops) = if self.nonlocal_dependencies.is_empty() {
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
                self.set_node_bounds_spatial(
                    id,
                    patch.new.map(|node| node.bounds),
                    self.fixed_translation_bounds(scene, id),
                );
            }
        }
        frame.version = Some(scene.version.get());
        let mut delta = RetainedFrameDelta {
            from_version: self.version.get(),
            to_version: scene.version.get(),
            patches: patches.into(),
            previous,
            depth,
            damage: backdrop_damage.into(),
            dirty_backdrops: dirty_backdrops.into(),
            backdrop_damage_complete: true,
            index: Rc::new(index),
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
        frame.dependency_free = self.dependency_free;
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
                    revision: NodeGeneration::new(node.generation),
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
            let index = retained_patch_index(&patches);
            let (previous, depth) = prune_shadowed_delta(frame.delta.clone(), &index);
            let mut delta = RetainedFrameDelta {
                from_version: self.version.get(),
                to_version: scene.version.get(),
                patches: patches.into(),
                previous,
                depth,
                damage: Rc::new([]),
                dirty_backdrops: Rc::new([]),
                backdrop_damage_complete: false,
                index: Rc::new(index),
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
        frame.version = Some(scene.version.get());
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
        frame.version = Some(self.version.get());
        let nodes = Rc::make_mut(&mut frame.nodes);
        for id in changed {
            let Some(metadata) = self.node_metadata.get(id) else {
                continue;
            };
            let Some(&index) = frame.node_index.get(id) else {
                continue;
            };
            nodes[index].revision = NodeGeneration::new(metadata.generation);
        }
    }

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
            let chunk = &self.chunks[&id];
            let old = frame.node_state(id);
            let (bounds, kind) = match &node.kind {
                NodeKind::Scene { .. } => {
                    let natural = self.influenced_bounds(scene, id, chunk.canvas.visual_bounds());
                    (
                        self.retained_output_bounds(scene, id, natural),
                        RetainedNodeKind::Scene,
                    )
                }
                NodeKind::Layer(_) => {
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
                    revision: NodeGeneration::new(node.generation),
                    bounds,
                    order: old.map_or(0, |node| node.order),
                    kind,
                    placement_bits: None,
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
        let index = retained_patch_index(&patches);
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
        frame.version = Some(scene.version.get());
        frame.delta = Some(Rc::new(RetainedFrameDelta {
            from_version: self.version.get(),
            to_version: scene.version.get(),
            patches: patches.into(),
            previous,
            depth,
            damage: explicit_damage.into(),
            dirty_backdrops: Rc::new([]),
            backdrop_damage_complete: false,
            index: Rc::new(index),
        }));
        let canvas = Rc::make_mut(&mut self.canvas);
        frame.invalidated_bounds = canvas.invalidated_bounds.clone();
        frame.invalidate_all = canvas.invalidate_all;
        frame.dependency_free = self.dependency_free;
        frame.requires_damage_propagation = !self.nonlocal_dependencies.is_empty();
        canvas.persistent_frame = Some(frame);
        true
    }

    pub(crate) fn rebuild_frame_override(&mut self, scene: &RetainedScene) {
        let canvas_bounds =
            Bounds::canvas(self.canvas.physical_width(), self.canvas.physical_height());
        let mut influences = HashMap::default();
        self.collect_bounds_influences(
            scene,
            scene.root,
            BoundsInfluence {
                outset: 0,
                clip: canvas_bounds,
            },
            &mut influences,
        );
        let leaf_bounds = scene
            .nodes
            .iter()
            .filter(|(_, node)| matches!(node.kind, NodeKind::Scene { .. }))
            .map(|(&id, _)| {
                let chunk = &self.chunks[&id];
                let natural = influences[&id].apply(chunk.canvas.visual_bounds());
                (id, self.retained_output_bounds(scene, id, natural))
            })
            .collect::<HashMap<_, _>>();
        let mut subtree_leaf_bounds = HashMap::default();
        self.collect_subtree_leaf_bounds(scene, scene.root, &leaf_bounds, &mut subtree_leaf_bounds);
        let mut nodes = Vec::with_capacity(self.chunks.len());
        self.collect_frame_nodes(
            scene,
            RetainedParent::content(scene.root),
            &leaf_bounds,
            &subtree_leaf_bounds,
            &influences,
            &mut nodes,
        );
        let node_index = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id, index))
            .collect();
        let canvas = Rc::make_mut(&mut self.canvas);
        canvas.persistent_frame = Some(crate::canvas::RetainedFrame {
            root: scene.root,
            logical_size: (scene.width, scene.height),
            physical_size: canvas.physical_size(),
            scale_bits: scene.scale.to_bits(),
            nodes: nodes.into(),
            node_index: Rc::new(node_index),
            state_pages: Rc::new(HashMap::default()),
            invalidated_bounds: canvas.invalidated_bounds.clone(),
            invalidate_all: canvas.invalidate_all,
            incremental_complete: true,
            version: Some(scene.version.get()),
            delta: None,
            dependency_free: self.dependency_free,
            requires_damage_propagation: !self.nonlocal_dependencies.is_empty(),
        });
    }

    pub(crate) fn collect_frame_nodes(
        &self,
        scene: &RetainedScene,
        parent: RetainedParent,
        leaf_bounds: &HashMap<RetainedNodeId, Bounds>,
        subtree_leaf_bounds: &HashMap<RetainedNodeId, Bounds>,
        influences: &HashMap<RetainedNodeId, BoundsInfluence>,
        nodes: &mut Vec<crate::canvas::RetainedNodeState>,
    ) {
        let children = scene.nodes[&parent.node]
            .children(parent.branch)
            .expect("validated retained branch");
        for &id in children.values() {
            let node = &scene.nodes[&id];
            match &node.kind {
                NodeKind::Group => {
                    self.collect_frame_nodes(
                        scene,
                        RetainedParent::content(id),
                        leaf_bounds,
                        subtree_leaf_bounds,
                        influences,
                        nodes,
                    );
                }
                NodeKind::Scene { .. } => {
                    nodes.push(crate::canvas::RetainedNodeState {
                        id,
                        revision: NodeGeneration::new(node.generation),
                        bounds: leaf_bounds[&id],
                        order: nodes.len() as u32,
                        kind: RetainedNodeKind::Scene,
                        placement_bits: None,
                    });
                }
                NodeKind::Layer(descriptor) => {
                    self.collect_frame_nodes(
                        scene,
                        RetainedParent::content(id),
                        leaf_bounds,
                        subtree_leaf_bounds,
                        influences,
                        nodes,
                    );
                    if matches!(descriptor, RetainedLayerDescriptor::Mask(_)) {
                        self.collect_frame_nodes(
                            scene,
                            RetainedParent::mask(id),
                            leaf_bounds,
                            subtree_leaf_bounds,
                            influences,
                            nodes,
                        );
                    }
                    let chunk = &self.chunks[&id];
                    let bounds = influences[&id]
                        .apply(chunk_layer_influence_bounds(chunk))
                        .union(subtree_leaf_bounds[&id]);
                    nodes.push(crate::canvas::RetainedNodeState {
                        id,
                        revision: NodeGeneration::new(node.generation),
                        bounds,
                        order: nodes.len() as u32,
                        kind: RetainedNodeKind::Layer,
                        placement_bits: None,
                    });
                }
            }
        }
    }

    pub(crate) fn collect_bounds_influences(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        inherited: BoundsInfluence,
        influences: &mut HashMap<RetainedNodeId, BoundsInfluence>,
    ) {
        influences.insert(id, inherited);
        let node = &scene.nodes[&id];
        let child_influence = if let NodeKind::Layer(layer) = &node.kind {
            inherited.with_nearer(self.local_bounds_influence(id, layer))
        } else {
            inherited
        };
        for child in node.content.values().chain(node.mask.values()) {
            self.collect_bounds_influences(scene, *child, child_influence, influences);
        }
    }

    pub(crate) fn local_bounds_influence(
        &self,
        id: RetainedNodeId,
        layer: &RetainedLayerDescriptor,
    ) -> BoundsInfluence {
        let canvas_bounds =
            Bounds::canvas(self.canvas.physical_width(), self.canvas.physical_height());
        match layer {
            RetainedLayerDescriptor::ClipPath { .. }
            | RetainedLayerDescriptor::ClipSdf { .. }
            | RetainedLayerDescriptor::Isolate { .. }
            | RetainedLayerDescriptor::Opacity { .. }
            | RetainedLayerDescriptor::Blend { .. } => BoundsInfluence {
                outset: 0,
                clip: self.chunks[&id]
                    .canvas
                    .draw_records
                    .first()
                    .map_or(canvas_bounds, |draw| {
                        let clip = draw.pixel_bounds;
                        Bounds::new(clip.x0, clip.y0, clip.x1, clip.y1)
                    }),
            },
            RetainedLayerDescriptor::Filter {
                filter: value,
                sample_region,
            } => {
                let outset = filter::filter_outset(value);
                BoundsInfluence {
                    outset,
                    // This is a conservative summary for disjoint input and dependency bounds;
                    // it may redraw an extra boundary tile but can never omit affected pixels.
                    clip: filter::region_bounds(sample_region)
                        .outset(filter::filter_dependency_outset(value))
                        .outset(outset)
                        .intersect(filter::unclipped_filtered_region_bounds(
                            value,
                            sample_region,
                        ))
                        .intersect(canvas_bounds),
                }
            }
            RetainedLayerDescriptor::Mask(mask) => BoundsInfluence {
                outset: 0,
                clip: filter::region_bounds(&mask.region),
            },
            RetainedLayerDescriptor::Backdrop { .. } => BoundsInfluence {
                outset: 0,
                clip: canvas_bounds,
            },
        }
    }

    pub(crate) fn collect_subtree_leaf_bounds(
        &self,
        scene: &RetainedScene,
        id: RetainedNodeId,
        leaf_bounds: &HashMap<RetainedNodeId, Bounds>,
        subtree: &mut HashMap<RetainedNodeId, Bounds>,
    ) -> Bounds {
        let node = &scene.nodes[&id];
        let bounds = match &node.kind {
            NodeKind::Scene { .. } => leaf_bounds[&id],
            NodeKind::Group | NodeKind::Layer(_) => node
                .content
                .values()
                .chain(node.mask.values())
                .copied()
                .map(|child| self.collect_subtree_leaf_bounds(scene, child, leaf_bounds, subtree))
                .fold(Bounds::new(0, 0, 0, 0), Bounds::union),
        };
        subtree.insert(id, bounds);
        bounds
    }

    pub(crate) fn rebuild_spatial_index(&mut self, scene: &RetainedScene) {
        self.node_bounds.clear();
        self.raw_node_bounds.clear();
        self.bounded_node_bounds.clear();
        self.bounded_raw_node_bounds.clear();
        let tiles_size = (
            Rc::make_mut(&mut self.canvas).width_in_tiles(),
            Rc::make_mut(&mut self.canvas).height_in_tiles(),
        );
        let tile_count = tiles_size.0 as usize * tiles_size.1 as usize;
        self.node_tiles.clear();
        self.node_tiles.resize(tile_count, HashSet::default());
        self.spatial_nodes.clear();
        self.raw_node_tiles.clear();
        self.raw_node_tiles.resize(tile_count, HashSet::default());
        self.spatial_tiles_size = tiles_size;
        let frame = Rc::make_mut(&mut self.canvas)
            .persistent_frame
            .clone()
            .expect("persistent frame override");
        for &node in frame.nodes.iter() {
            self.set_node_bounds_spatial(
                node.id,
                Some(node.bounds),
                self.fixed_translation_bounds(scene, node.id),
            );
        }
        let raw_bounds = self
            .chunks
            .iter()
            .filter_map(|(&id, chunk)| {
                // Removed chunks are reclaimed later in the update after their old plan metadata
                // has been inspected. A resize can rebuild this index first, so derive it only
                // from nodes that are live in the new scene. This fixes the update ordering at
                // its source instead of special-casing resize transactions in callers.
                let node = scene.nodes.get(&id)?;
                let bounds = match node.kind {
                    NodeKind::Layer(_) => chunk_layer_influence_bounds(chunk),
                    NodeKind::Scene { .. } => chunk.canvas.visual_bounds(),
                    NodeKind::Group => unreachable!("groups do not own chunks"),
                };
                Some((id, bounds))
            })
            .collect::<Vec<_>>();
        for (id, bounds) in raw_bounds {
            self.set_raw_node_bounds_spatial(
                id,
                Some(bounds),
                self.fixed_translation_bounds(scene, id),
            );
        }
    }

    /// Refreshes only changed spatial entries after a non-structural frame-state fallback.
    ///
    /// Rebuilding `RetainedFrame` can be cheaper than proving a dependency-aware delta, but that
    /// does not invalidate the unchanged nodes' tile memberships. Reinitializing every tile
    /// HashSet here made a full-screen bounded chart translation pay O(surface tiles) each frame.
    pub(crate) fn patch_changed_spatial_index(
        &mut self,
        scene: &RetainedScene,
        changed: &HashSet<RetainedNodeId>,
        removed: &HashSet<RetainedNodeId>,
    ) -> bool {
        if self.spatial_tiles_size != (self.canvas.width_in_tiles(), self.canvas.height_in_tiles())
        {
            return false;
        }
        let Some(frame) = self.canvas.persistent_frame.clone() else {
            return false;
        };
        for &id in removed {
            self.set_node_bounds_spatial(id, None, None);
            self.set_raw_node_bounds_spatial(id, None, None);
        }
        for &id in changed {
            let Some(node) = scene.nodes.get(&id) else {
                return false;
            };
            if matches!(node.kind, NodeKind::Group) {
                continue;
            }
            let Some(state) = frame.node_state(id) else {
                return false;
            };
            let Some(chunk) = self.chunks.get(&id) else {
                return false;
            };
            let fixed = self.fixed_translation_bounds(scene, id);
            let raw = if let Some(fixed) = fixed {
                self.raw_node_bounds.get(&id).copied().unwrap_or(fixed)
            } else {
                match node.kind {
                    NodeKind::Layer(_) => chunk_layer_influence_bounds(chunk),
                    NodeKind::Scene { .. } => chunk.canvas.visual_bounds(),
                    NodeKind::Group => unreachable!("groups do not own chunks"),
                }
            };
            self.set_node_bounds_spatial(id, Some(state.bounds), fixed);
            self.set_raw_node_bounds_spatial(id, Some(raw), fixed);
        }
        true
    }

    pub(crate) fn set_node_bounds_spatial(
        &mut self,
        id: RetainedNodeId,
        bounds: Option<Bounds>,
        fixed_spatial_bounds: Option<Bounds>,
    ) {
        let bounds = bounds.filter(|bounds| !bounds.is_empty());
        let fixed_spatial_bounds = fixed_spatial_bounds.filter(|bounds| !bounds.is_empty());
        let was_spatial = self.spatial_nodes.contains(&id);
        let should_be_spatial = bounds.is_some() && self.painter_bases.contains_key(&id);
        if self.node_bounds.get(&id).copied() == bounds
            && self.bounded_node_bounds.get(&id).copied() == fixed_spatial_bounds
            && was_spatial == should_be_spatial
        {
            return;
        }
        let old_fixed = self.bounded_node_bounds.remove(&id);
        let old = match self.node_bounds.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let old = *entry.get();
                if Some(old) != bounds {
                    if let Some(bounds) = bounds {
                        *entry.get_mut() = bounds;
                    } else {
                        entry.remove();
                    }
                }
                Some(old)
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                if let Some(bounds) = bounds {
                    entry.insert(bounds);
                }
                None
            }
        };
        if was_spatial
            && old_fixed.is_none()
            && let Some(old) = old
        {
            for tile in self.tiles_for_bounds(old) {
                self.node_tiles[tile].remove(&id);
            }
        }
        let is_spatial = should_be_spatial;
        if is_spatial && let Some(bounds) = bounds {
            if let Some(fixed) = fixed_spatial_bounds {
                self.bounded_node_bounds.insert(id, fixed);
            } else {
                for tile in self.tiles_for_bounds(bounds) {
                    self.node_tiles[tile].insert(id);
                }
            }
            self.spatial_nodes.insert(id);
        } else {
            self.spatial_nodes.remove(&id);
        }
    }

    pub(crate) fn set_raw_node_bounds_spatial(
        &mut self,
        id: RetainedNodeId,
        bounds: Option<Bounds>,
        fixed_spatial_bounds: Option<Bounds>,
    ) {
        let bounds = bounds.filter(|bounds| !bounds.is_empty());
        let fixed_spatial_bounds = fixed_spatial_bounds.filter(|bounds| !bounds.is_empty());
        if self.raw_node_bounds.get(&id).copied() == bounds
            && self.bounded_raw_node_bounds.get(&id).copied() == fixed_spatial_bounds
        {
            return;
        }
        let old_fixed = self.bounded_raw_node_bounds.remove(&id);
        let old = match self.raw_node_bounds.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let old = *entry.get();
                if Some(old) != bounds {
                    if let Some(bounds) = bounds {
                        *entry.get_mut() = bounds;
                    } else {
                        entry.remove();
                    }
                }
                Some(old)
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                if let Some(bounds) = bounds {
                    entry.insert(bounds);
                }
                None
            }
        };
        if old_fixed.is_none()
            && let Some(old) = old
        {
            for tile in self.tiles_for_bounds(old) {
                self.raw_node_tiles[tile].remove(&id);
            }
        }
        if let Some(bounds) = bounds {
            if let Some(fixed) = fixed_spatial_bounds {
                self.bounded_raw_node_bounds.insert(id, fixed);
            } else {
                for tile in self.tiles_for_bounds(bounds) {
                    self.raw_node_tiles[tile].insert(id);
                }
            }
        }
    }

    pub(crate) fn tiles_for_bounds(&self, bounds: Bounds) -> super::spatial_tiles::BoundsTileIter {
        super::spatial_tiles::BoundsTileIter::new(
            bounds,
            self.canvas.width_in_tiles(),
            self.canvas.height_in_tiles(),
        )
    }

    pub(crate) fn spatial_candidates(&self, bounds: Bounds) -> HashSet<RetainedNodeId> {
        let mut candidates = HashSet::default();
        for tile in self.tiles_for_bounds(bounds) {
            candidates.extend(self.node_tiles[tile].iter().copied());
        }
        candidates.extend(
            self.bounded_node_bounds
                .iter()
                .filter_map(|(&id, &fixed)| (!fixed.intersect(bounds).is_empty()).then_some(id)),
        );
        candidates
    }

    pub(crate) fn raw_spatial_candidates(&self, bounds: Bounds) -> HashSet<RetainedNodeId> {
        let mut candidates = HashSet::default();
        for tile in self.tiles_for_bounds(bounds) {
            candidates.extend(self.raw_node_tiles[tile].iter().copied());
        }
        candidates.extend(
            self.bounded_raw_node_bounds
                .iter()
                .filter_map(|(&id, &fixed)| (!fixed.intersect(bounds).is_empty()).then_some(id)),
        );
        candidates
    }

    pub(crate) fn root_reorder_damage(
        &self,
        old_bases: &HashMap<RetainedNodeId, Rc<[u128]>>,
        changed: &HashSet<RetainedNodeId>,
    ) -> Vec<(RetainedNodeId, Bounds)> {
        let mut damage = HashMap::<RetainedNodeId, Bounds>::default();
        for &id in changed {
            let (Some(&bounds), Some(old), Some(new)) = (
                self.node_bounds.get(&id),
                old_bases.get(&id),
                self.painter_bases.get(&id),
            ) else {
                continue;
            };
            for candidate in self.spatial_candidates(bounds) {
                if candidate == id || !self.node_bounds.contains_key(&candidate) {
                    continue;
                }
                let Some(candidate_new) = self.painter_bases.get(&candidate) else {
                    continue;
                };
                let candidate_old = old_bases.get(&candidate).unwrap_or(candidate_new);
                if (old.as_ref() < candidate_old.as_ref())
                    == (new.as_ref() < candidate_new.as_ref())
                {
                    continue;
                }
                let overlap = bounds.intersect(self.node_bounds[&candidate]);
                if !overlap.is_empty() {
                    damage
                        .entry(id)
                        .and_modify(|current| *current = current.union(overlap))
                        .or_insert(overlap);
                }
            }
        }
        damage.into_iter().collect()
    }

    pub(crate) fn push_command_list(&mut self) -> usize {
        let canvas = Rc::make_mut(&mut self.canvas);
        let id = canvas.command_lists.len();
        canvas.command_lists.push(CommandList::default());
        id
    }
}
