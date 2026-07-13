use super::helpers::*;
use super::*;

impl PersistentSceneMaterializer {
    pub(crate) fn encode_node(scene: &RetainedScene, node: &SceneNode) -> Canvas {
        let mut canvas = Canvas::new(scene.width, scene.height, scene.scale);
        Self::encode_node_into(scene, node, &mut canvas);
        canvas
    }

    pub(crate) fn encode_node_into(scene: &RetainedScene, node: &SceneNode, canvas: &mut Canvas) {
        canvas.resize_surface(scene.width, scene.height);
        canvas.reset();
        match &node.kind {
            NodeKind::Scene {
                canvas: child,
                transform,
                ..
            } => {
                canvas.append(child, Point::ZERO);
                canvas.set_retained_transform(*transform);
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
                        canvas.push_mask_layer(
                            Canvas::new(scene.width, scene.height, scene.scale),
                            mask.clone(),
                        );
                    }
                }
                canvas.pop_layer();
            }
            NodeKind::Group => {}
        }
    }

    pub(crate) fn insert_glyphs(
        arenas: &mut MaterializedArenas,
        glyphs: &[CanvasGlyph],
    ) -> Option<ArenaAllocation> {
        let first = glyphs.first().copied()?;
        let arena = arenas.glyphs.get_or_insert_with(|| SceneArena::new(first));
        Some(arena.insert(glyphs))
    }

    pub(crate) fn replace_glyphs(
        arenas: &mut MaterializedArenas,
        allocation: &mut Option<ArenaAllocation>,
        glyphs: &[CanvasGlyph],
    ) -> bool {
        match (*allocation, glyphs.first().copied()) {
            (Some(id), Some(first)) => arenas
                .glyphs
                .get_or_insert_with(|| SceneArena::new(first))
                .replace(id, glyphs),
            (Some(id), None) => {
                arenas.glyphs.as_mut().unwrap().remove(id);
                *allocation = None;
                true
            }
            (None, Some(_)) => {
                *allocation = Self::insert_glyphs(arenas, glyphs);
                true
            }
            (None, None) => false,
        }
    }

    pub(crate) fn remove_chunk(&mut self, chunk: SceneChunk) {
        Self::remove_chunk_resources(
            &mut self.resource_refs,
            &mut self.canvas,
            &chunk.canvas.scene_images,
        );
        self.arenas.lines.remove(chunk.lines);
        self.arenas.paths.remove(chunk.paths);
        self.arenas.draws.remove(chunk.draws);
        self.arenas.brushes.remove(chunk.brushes);
        self.arenas.sdfs.remove(chunk.sdfs);
        self.arenas.shadows.remove(chunk.shadows);
        if let Some(glyphs) = chunk.glyphs {
            self.arenas.glyphs.as_mut().unwrap().remove(glyphs);
        }
        self.arenas.runs.remove(chunk.runs.unwrap());
        self.arenas.backdrops.remove(chunk.backdrops);
        self.arenas.segments.remove(chunk.segments);
    }

    pub(crate) fn add_chunk_resources(
        resource_refs: &mut HashMap<ImageKey, (Rc<Image>, usize)>,
        canvas: &mut Rc<Canvas>,
        resources: &ImageResourceStore,
    ) {
        for (key, image) in resources.iter() {
            let entry = resource_refs
                .entry(key)
                .or_insert_with(|| (image.clone(), 0));
            entry.0 = image.clone();
            entry.1 += 1;
            Rc::make_mut(canvas).scene_images.insert(key, image.clone());
        }
    }

    pub(crate) fn remove_chunk_resources(
        resource_refs: &mut HashMap<ImageKey, (Rc<Image>, usize)>,
        canvas: &mut Rc<Canvas>,
        resources: &ImageResourceStore,
    ) {
        for (key, _) in resources.iter() {
            let remove = resource_refs.get_mut(&key).is_some_and(|(_, count)| {
                *count -= 1;
                *count == 0
            });
            if remove {
                resource_refs.remove(&key);
                Rc::make_mut(canvas).scene_images.remove(key);
            }
        }
    }

    pub(crate) fn arena_compactions(&self) -> u64 {
        self.arenas.lines.compactions()
            + self.arenas.paths.compactions()
            + self.arenas.draws.compactions()
            + self.arenas.brushes.compactions()
            + self.arenas.sdfs.compactions()
            + self.arenas.shadows.compactions()
            + self
                .arenas
                .glyphs
                .as_ref()
                .map_or(0, SceneArena::compactions)
            + self.arenas.runs.compactions()
            + self.arenas.backdrops.compactions()
            + self.arenas.segments.compactions()
    }

    /// Path compaction changes line-to-path references, while glyph compaction changes run glyph
    /// bases. Compaction of every other arena only requires path/draw record remapping.
    pub(crate) fn immutable_remap_compactions(&self) -> u64 {
        self.arenas.paths.compactions()
            + self
                .arenas
                .glyphs
                .as_ref()
                .map_or(0, SceneArena::compactions)
    }

    pub(crate) fn remap_all_chunks(&mut self, include_immutable_geometry: bool) {
        let Self { chunks, arenas, .. } = self;
        for chunk in chunks.values() {
            Self::remap_chunk_data(arenas, chunk, include_immutable_geometry);
        }
    }

    pub(crate) fn sync_canvas_data(&mut self, chunks_rebuilt: u32, full_scene_sync: bool) {
        let (arena_live_bytes, arena_capacity_bytes) = self.arena_usage();
        let arena_fragmentation = if arena_capacity_bytes == 0 {
            0.0
        } else {
            1.0 - arena_live_bytes as f32 / arena_capacity_bytes as f32
        };
        let arena_compactions = self.arena_compactions();
        let canvas = Rc::make_mut(&mut self.canvas);
        let lines = sync_arena(&mut canvas.lines, &mut self.arenas.lines, Line::default());
        let paths = sync_arena(
            &mut canvas.path_records,
            &mut self.arenas.paths,
            PathRecord::default(),
        );
        let draws = sync_arena(
            &mut canvas.draw_records,
            &mut self.arenas.draws,
            inactive_draw(),
        );
        let brushes = sync_arena(&mut canvas.brush_blob, &mut self.arenas.brushes, 0);
        let sdfs = sync_arena(&mut canvas.sdf_blob, &mut self.arenas.sdfs, 0);
        let shadows = sync_arena(&mut canvas.sdf_shadow_blob, &mut self.arenas.shadows, 0);
        let glyphs = if let Some(glyphs) = &mut self.arenas.glyphs {
            let vacant = glyphs
                .values()
                .first()
                .copied()
                .expect("glyph arena is initialized by a glyph");
            sync_arena(&mut canvas.text_glyphs, glyphs, vacant)
        } else {
            canvas.text_glyphs.clear();
            Vec::new()
        };
        let text_runs = sync_arena(
            &mut canvas.text_runs,
            &mut self.arenas.runs,
            TextRun {
                glyph_start: 0,
                glyph_count: 0,
            },
        );
        canvas.path_cnt = self.arenas.paths.values().len() as u32;
        canvas.backdrop_pool_capacity = self.arenas.backdrops.values().len() as u32;
        canvas.tile_cnt = self.arenas.segments.values().len() as u32;
        let cpu_copied_bytes = range_bytes::<Line>(&lines)
            + range_bytes::<PathRecord>(&paths)
            + range_bytes::<DrawRecord>(&draws)
            + range_bytes::<u32>(&brushes)
            + range_bytes::<u32>(&sdfs)
            + range_bytes::<u32>(&shadows)
            + range_bytes::<CanvasGlyph>(&glyphs)
            + range_bytes::<TextRun>(&text_runs);
        canvas.buffer_changes = Some(SceneBufferChanges {
            lines,
            paths,
            draws,
            brushes,
            sdfs,
            shadows,
            glyphs,
            text_runs,
            chunks_rebuilt,
            plan_fragments_rebuilt: 0,
            full_scene_sync,
            surface_changed: false,
            cpu_copied_bytes,
            painter: Vec::new(),
            plan_structure_reused: false,
            plan_values_patched: false,
            plan_layer_stack: Vec::new(),
            filter_resources_changed: false,
            arena_live_bytes,
            arena_capacity_bytes,
            arena_fragmentation,
            arena_compactions,
        });
    }

    pub(crate) fn arena_usage(&self) -> (u64, u64) {
        let mut live = 0;
        let mut capacity = 0;
        macro_rules! add {
            ($arena:expr, $ty:ty) => {{
                live += ($arena.live_len() * std::mem::size_of::<$ty>()) as u64;
                capacity += ($arena.values().len() * std::mem::size_of::<$ty>()) as u64;
            }};
        }
        add!(self.arenas.lines, Line);
        add!(self.arenas.paths, PathRecord);
        add!(self.arenas.draws, DrawRecord);
        add!(self.arenas.brushes, u32);
        add!(self.arenas.sdfs, u32);
        add!(self.arenas.shadows, u32);
        if let Some(glyphs) = &self.arenas.glyphs {
            add!(glyphs, CanvasGlyph);
        }
        add!(self.arenas.runs, TextRun);
        add!(self.arenas.backdrops, u32);
        add!(self.arenas.segments, u32);
        (live, capacity)
    }
}
