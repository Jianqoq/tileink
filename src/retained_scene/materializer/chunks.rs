//! Retained chunk encoding, allocation and physical-record remapping.
use super::helpers::*;
use super::*;

impl PersistentSceneMaterializer {
    /// Applies a same-scale viewport resize without re-encoding retained node geometry.
    ///
    /// Path backdrop and segment allocations depend on viewport clipping, so only those
    /// allocations are resized and remapped. SDF, image, and text chunks keep their stable slots.
    /// Changed and removed chunks must be processed first: resizing their stale previous-frame
    /// geometry can shrink shared scan allocations before their replacement is installed and
    /// publish an internally inconsistent frame. The regular update tail recompiles the
    /// surface-dependent plan and damage metadata.
    pub(crate) fn resize_surface_chunks(
        &mut self,
        scene: &RetainedScene,
        painter_nodes: &mut Vec<RetainedNodeId>,
    ) {
        let Self { chunks, arenas, .. } = self;
        for (&id, chunk) in chunks.iter_mut() {
            if chunk.canvas.logical_size() == (scene.width, scene.height) {
                continue;
            }
            let old_backdrops = chunk.canvas.backdrop_pool_capacity as usize;
            let old_segments = chunk.canvas.tile_cnt as usize;
            chunk
                .canvas
                .edit()
                .resize_surface(scene.width, scene.height);
            let new_backdrops = chunk.canvas.backdrop_pool_capacity as usize;
            let new_segments = chunk.canvas.tile_cnt as usize;
            if old_backdrops != 0 || new_backdrops != 0 {
                arenas.backdrops.resize(chunk.backdrops, new_backdrops);
            }
            if old_segments != 0 || new_segments != 0 {
                arenas.segments.resize(chunk.segments, new_segments);
            }
            if !chunk.canvas.path_records.is_empty() {
                Self::remap_chunk_data(arenas, chunk, false);
                // Resizing path scan allocations rewrites DrawRecord offsets without changing
                // painter identity. Include the owner in the normal metadata restore pass so
                // dirty GPU ranges are not mistaken for removed draws.
                painter_nodes.push(id);
            }
        }
    }

    /// Re-encodes one node and classifies whether its execution plan needs synchronization.
    pub(crate) fn rebuild_node(
        &mut self,
        scene: &RetainedScene,
        id: RetainedNodeId,
    ) -> NodeRebuild {
        let node = &scene.nodes[&id];
        let placement = match &node.kind {
            NodeKind::Scene {
                canvas, transform, ..
            } => Some(ScenePlacement {
                source: canvas.clone(),
                transform_bits: transform.as_coeffs().map(f64::to_bits),
            }),
            _ => None,
        };
        let updated = {
            // Borrow the materializer fields independently so an existing chunk stays in its map
            // slot while encoding, arena remapping, and resource refcounts are updated.
            let Self {
                canvas,
                chunks,
                arenas,
                resource_refs,
                ..
            } = self;
            chunks.get_mut(&id).map(|chunk| {
                if let Some((old, new)) = chunk.placement.as_mut().zip(placement.as_ref())
                    && Rc::ptr_eq(&old.source, &new.source)
                    && old.transform_bits != new.transform_bits
                {
                    let transform = Affine::new(new.transform_bits.map(f64::from_bits));
                    chunk.canvas.edit().set_retained_transform(transform);
                    let old_plan = chunk.plan_fingerprint;
                    let new_plan = chunk.canvas.execution_plan_fingerprint();
                    let mut moved = false;
                    if !chunk.canvas.path_records.is_empty() {
                        moved |= arenas.backdrops.resize(
                            chunk.backdrops,
                            chunk.canvas.backdrop_pool_capacity as usize,
                        );
                        moved |= arenas
                            .segments
                            .resize(chunk.segments, chunk.canvas.tile_cnt as usize);
                    }
                    chunk.instance = node.instance;
                    chunk.generation = node.generation;
                    old.transform_bits = new.transform_bits;
                    if old_plan != new_plan {
                        let metadata = chunk_plan_metadata(&chunk.canvas);
                        chunk.plain_fragment = metadata.plain_fragment;
                        chunk.local_draw_order = metadata.local_draw_order;
                    }
                    chunk.plan_fingerprint = new_plan;
                    if !chunk.backdrop_dependencies.is_empty() {
                        chunk.backdrop_dependencies = backdrop_dependencies(&chunk.canvas);
                    }
                    // Only transform-bearing path/draw records and scan allocation metadata
                    // changed. Local lines, paint/SDF blobs, glyphs, and runs remain immutable.
                    Self::remap_chunk_data(arenas, chunk, false);
                    return (
                        NodeRebuild {
                            plan_dirty: moved || old_plan != new_plan,
                            transform_only: !moved,
                        },
                        None,
                        false,
                    );
                }

                let old_plan = chunk.plan_fingerprint;
                let old_nonlocal = !chunk.backdrop_dependencies.is_empty();
                let old_lengths = SceneChunkLengths::from_canvas(&chunk.canvas);
                Self::remove_chunk_resources(resource_refs, canvas, &chunk.canvas.scene_images);
                Self::encode_node_into(scene, node, chunk.canvas.edit());
                let encoded = &chunk.canvas;
                let new_lengths = SceneChunkLengths::from_canvas(encoded);
                let new_plan = encoded.execution_plan_fingerprint();
                let mut moved = false;
                if old_lengths.lines != 0 || new_lengths.lines != 0 {
                    moved |= arenas.lines.resize(chunk.lines, new_lengths.lines);
                }
                if old_lengths.paths != 0 || new_lengths.paths != 0 {
                    moved |= arenas.paths.resize(chunk.paths, new_lengths.paths);
                }
                if old_lengths.draws != 0 || new_lengths.draws != 0 {
                    moved |= arenas.draws.resize(chunk.draws, new_lengths.draws);
                }
                if old_lengths.brushes != 0 || new_lengths.brushes != 0 {
                    moved |= arenas.brushes.replace(chunk.brushes, &encoded.brush_blob);
                }
                if old_lengths.sdfs != 0 || new_lengths.sdfs != 0 {
                    moved |= arenas.sdfs.replace(chunk.sdfs, &encoded.sdf_blob);
                }
                if old_lengths.shadows != 0 || new_lengths.shadows != 0 {
                    moved |= arenas
                        .shadows
                        .replace(chunk.shadows, &encoded.sdf_shadow_blob);
                }
                moved |= Self::replace_glyphs(arenas, &mut chunk.glyphs, &encoded.text_glyphs);
                if old_lengths.runs != 0 || new_lengths.runs != 0 {
                    moved |= arenas.runs.resize(chunk.runs, new_lengths.runs);
                }
                if old_lengths.backdrops != 0 || new_lengths.backdrops != 0 {
                    moved |=
                        arenas
                            .backdrops
                            .replace_filled(chunk.backdrops, new_lengths.backdrops, 0);
                }
                if old_lengths.segments != 0 || new_lengths.segments != 0 {
                    moved |=
                        arenas
                            .segments
                            .replace_filled(chunk.segments, new_lengths.segments, 0);
                }
                chunk.instance = node.instance;
                chunk.generation = node.generation;
                chunk.placement = placement.clone();
                if old_plan != new_plan {
                    let metadata = chunk_plan_metadata(&chunk.canvas);
                    chunk.plain_fragment = metadata.plain_fragment;
                    chunk.local_draw_order = metadata.local_draw_order;
                }
                chunk.plan_fingerprint = new_plan;
                // An unchanged command topology cannot introduce a Backdrop. Existing
                // Backdrops still refresh their bounds when their content changes.
                if old_plan != new_plan || old_nonlocal {
                    chunk.backdrop_dependencies = backdrop_dependencies(&chunk.canvas);
                }
                let new_nonlocal = !chunk.backdrop_dependencies.is_empty();
                Self::add_chunk_resources(resource_refs, canvas, &chunk.canvas.scene_images);
                Self::remap_chunk_data(arenas, chunk, true);
                // Geometry can change within an existing dependency domain, but membership
                // changes only when backdrop presence or command topology changes. Avoid
                // two hash-index updates for every ordinary content revision.
                (
                    NodeRebuild {
                        plan_dirty: moved || old_plan != new_plan,
                        transform_only: false,
                    },
                    (old_nonlocal != new_nonlocal).then_some(new_nonlocal),
                    old_plan != new_plan,
                )
            })
        };
        if let Some((updated, nonlocal_changed, plan_changed)) = updated {
            if let Some(nonlocal) = nonlocal_changed {
                if nonlocal {
                    self.nonlocal_dependencies.insert(id);
                } else {
                    self.nonlocal_dependencies.remove(&id);
                }
            }
            if plan_changed {
                if chunk_has_surface_dependent_plan(&self.chunks[&id].canvas) {
                    self.surface_dependent_plans.insert(id);
                } else {
                    self.surface_dependent_plans.remove(&id);
                }
            }
            return updated;
        }

        let encoded = Self::encode_node(scene, node);
        let glyphs = Self::insert_glyphs(&mut self.arenas, &encoded.text_glyphs);
        let plan_fingerprint = encoded.execution_plan_fingerprint();
        let plan_metadata = chunk_plan_metadata(&encoded);
        let chunk = SceneChunk {
            instance: node.instance,
            generation: node.generation,
            placement,
            lines: self.arenas.lines.insert(&encoded.lines),
            paths: self.arenas.paths.insert(&encoded.path_records),
            draws: self.arenas.draws.insert(&encoded.draw_records),
            brushes: self.arenas.brushes.insert(&encoded.brush_blob),
            sdfs: self.arenas.sdfs.insert(&encoded.sdf_blob),
            shadows: self.arenas.shadows.insert(&encoded.sdf_shadow_blob),
            glyphs,
            runs: self.arenas.runs.insert(&encoded.text_runs),
            backdrops: self
                .arenas
                .backdrops
                .insert_filled(encoded.backdrop_pool_capacity as usize, 0),
            segments: self
                .arenas
                .segments
                .insert_filled(encoded.tile_cnt as usize, 0),
            plan_fingerprint,
            plain_fragment: plan_metadata.plain_fragment,
            local_draw_order: plan_metadata.local_draw_order,
            backdrop_dependencies: backdrop_dependencies(&encoded),
            canvas: encoded.into(),
        };
        Self::add_chunk_resources(
            &mut self.resource_refs,
            &mut self.canvas,
            &chunk.canvas.scene_images,
        );
        Self::remap_chunk_data(&mut self.arenas, &chunk, true);
        self.chunks.insert(id, chunk);
        self.refresh_chunk_dependencies(id);
        NodeRebuild {
            plan_dirty: true,
            transform_only: false,
        }
    }

    pub(crate) fn refresh_chunk_dependencies(&mut self, id: RetainedNodeId) {
        let chunk = &self.chunks[&id];
        if !chunk.backdrop_dependencies.is_empty() {
            self.nonlocal_dependencies.insert(id);
        } else {
            self.nonlocal_dependencies.remove(&id);
        }
        if chunk_has_surface_dependent_plan(&chunk.canvas) {
            self.surface_dependent_plans.insert(id);
        } else {
            self.surface_dependent_plans.remove(&id);
        }
    }

    pub(crate) fn remap_chunk_data(
        arenas: &mut MaterializedArenas,
        chunk: &SceneChunk,
        include_immutable_geometry: bool,
    ) {
        let line_base = if chunk.canvas.lines.is_empty() {
            0
        } else {
            arenas.lines.range(chunk.lines).start as u32
        };
        let path_base = if chunk.canvas.path_records.is_empty() {
            0
        } else {
            arenas.paths.range(chunk.paths).start as u32
        };
        let brush_base = if chunk.canvas.brush_blob.is_empty() {
            0
        } else {
            arenas.brushes.range(chunk.brushes).start as u32
        };
        let sdf_base = if chunk.canvas.sdf_blob.is_empty() {
            0
        } else {
            arenas.sdfs.range(chunk.sdfs).start as u32
        };
        let shadow_base = if chunk.canvas.sdf_shadow_blob.is_empty() {
            0
        } else {
            arenas.shadows.range(chunk.shadows).start as u32
        };
        let glyph_base = chunk.glyphs.map_or(0, |id| {
            arenas.glyphs.as_ref().unwrap().range(id).start as u32
        });
        let run_base = if chunk.canvas.text_runs.is_empty() {
            0
        } else {
            arenas.runs.range(chunk.runs).start as u32
        };
        let backdrop_base = if chunk.canvas.backdrop_pool_capacity == 0 {
            0
        } else {
            arenas.backdrops.range(chunk.backdrops).start as u32
        };
        let segment_base = if chunk.canvas.tile_cnt == 0 {
            0
        } else {
            arenas.segments.range(chunk.segments).start as u32
        };

        if include_immutable_geometry && !chunk.canvas.lines.is_empty() {
            arenas
                .lines
                .write_mapped(chunk.lines, &chunk.canvas.lines, |mut line| {
                    line.path_id = line.path_id.saturating_add(path_base);
                    line
                });
        }

        if !chunk.canvas.path_records.is_empty() {
            arenas
                .paths
                .write_mapped(chunk.paths, &chunk.canvas.path_records, |mut path| {
                    path.path_id = path.path_id.saturating_add(path_base);
                    path.line_start = path.line_start.saturating_add(line_base);
                    path.data_offset = path.data_offset.saturating_add(backdrop_base);
                    path.segment_start = path.segment_start.saturating_add(segment_base);
                    path
                });
        }

        if !chunk.canvas.draw_records.is_empty() {
            arenas
                .draws
                .write_mapped(chunk.draws, &chunk.canvas.draw_records, |mut draw| {
                    if draw.path_id != DrawRecord::NONE {
                        draw.path_id = draw.path_id.saturating_add(path_base);
                    }
                    if draw.glyph_run_id != DrawRecord::NONE {
                        draw.glyph_run_id = draw.glyph_run_id.saturating_add(run_base);
                    }
                    if draw.brush_offset != DrawRecord::NONE {
                        draw.brush_offset = draw.brush_offset.saturating_add(brush_base);
                    }
                    if draw.sdf_offset != DrawRecord::NONE {
                        draw.sdf_offset = draw.sdf_offset.saturating_add(sdf_base);
                    }
                    if draw.sdf_shadow_offset != DrawRecord::NONE {
                        draw.sdf_shadow_offset = draw.sdf_shadow_offset.saturating_add(shadow_base);
                    }
                    draw
                });
        }
        if include_immutable_geometry && !chunk.canvas.text_runs.is_empty() {
            arenas
                .runs
                .write_mapped(chunk.runs, &chunk.canvas.text_runs, |run| TextRun {
                    glyph_start: run.glyph_start.saturating_add(glyph_base),
                    glyph_count: run.glyph_count,
                });
        }
    }
}
