#![cfg_attr(not(feature = "wgpu"), allow(dead_code))]

use std::{collections::HashSet, ops::Range};

use bytemuck::{Pod, Zeroable};

use crate::{
    canvas::{Canvas, PainterKey},
    shared::{
        bounds::{Bounds, PixelBounds, TileBbox},
        draw_record::{DrawRecord, DrawTag},
        execution::{ExecOp, ExecPlan, LayerStackEntry},
        gpu_coarse::TileDrawRecord,
        layer::{
            Layer,
            filter::{Filter, FilterInput, FilterPrimitive, FilterPrimitiveKind},
        },
        path::PathRecord,
        scene_arena::{ArenaAllocation, SceneArena},
    },
    text::PreparedTextData,
};

pub(crate) const SCAN_CHUNK_SIZE: u32 = 256;
pub(crate) const CUMSUM_CHUNK_SIZE: u32 = 256;
pub(crate) const COARSE_CHUNK_SIZE: u32 = 256;
pub(crate) const TILE_DRAW_PAGE_WORDS: usize = COARSE_CHUNK_SIZE as usize + 1;
const TILE_DRAW_FLAT_FLAG: u32 = 1 << 31;
pub(crate) const FINE_WORKGROUP_SIZE: u32 = 256;
pub(crate) const FINE_LOCAL_CLIP_DEPTH: usize = 4;
pub(crate) const FINE_LOCAL_GROUP_DEPTH: usize = 2;
pub(crate) const FINE_GROUP_SPILL_FIELDS: usize = 5;

/// Canvas-derived fixed capacities for GPU buffers.
///
/// GPU compute stages cannot grow vectors while dispatching. This plan keeps
/// allocation sizes explicit and shared by native wgpu upload paths
/// so both backends launch against the same buffer contract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GpuBufferLengths {
    pub line_count: usize,
    pub path_count: usize,
    pub draw_count: usize,
    pub backdrop_record_count: usize,
    pub backdrop_len: usize,
    pub segment_capacity: usize,
    pub scan_chunk_count: usize,
    pub cumsum_chunk_count: usize,
    pub cumsum_row_count: usize,
    pub coarse_chunk_count: usize,
    pub coarse_ptcl_capacity: usize,
    pub coarse_glyph_capacity: usize,
    pub tile_draw_index_count: usize,
    pub tile_draw_chunk_count: usize,
    pub text_enabled: bool,
    pub text_run_count: usize,
    pub text_glyph_count: usize,
    pub tiles_width: usize,
    pub tiles_height: usize,
    pub tile_count: usize,
    pub image_pixels: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GpuPathPlanCounts {
    pub(crate) scan_chunks: usize,
    pub(crate) cumsum_chunks: usize,
    pub(crate) cumsum_rows: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GpuLengthOverrides {
    pub(crate) path_plan_counts: Option<GpuPathPlanCounts>,
    pub(crate) coarse_glyph_capacity: Option<usize>,
    pub(crate) coarse_ptcl_capacity: Option<usize>,
    pub(crate) cached_stack_depths: Option<(usize, usize)>,
}

impl GpuBufferLengths {
    #[cfg(test)]
    pub(crate) fn from_scene(canvas: &Canvas) -> Self {
        Self::from_scene_with_text(canvas, None)
    }

    #[cfg(test)]
    pub(crate) fn from_scene_with_text(canvas: &Canvas, text: Option<&PreparedTextData>) -> Self {
        let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let tiles_width = canvas.width_in_tiles() as usize;
        let tiles_height = canvas.height_in_tiles() as usize;
        let tile_draw_counts = tile_draw_counts_for_order(
            &canvas.draw_records,
            &plan.draw_order,
            (tiles_width as u32, tiles_height as u32),
        );
        Self::from_scene_with_text_and_tile_draw_counts(
            canvas,
            text,
            tiles_width,
            tiles_height,
            tile_draw_counts,
            GpuLengthOverrides::default(),
        )
    }

    pub(crate) fn from_scene_with_text_and_tile_draw_bins(
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        plan: &ExecPlan,
        bins: &mut TileDrawBins,
        cursors: &mut Vec<u32>,
        incremental: bool,
        mut overrides: GpuLengthOverrides,
    ) -> Self {
        let tiles_width = canvas.width_in_tiles() as usize;
        let tiles_height = canvas.height_in_tiles() as usize;
        let tiles_size = (tiles_width as u32, tiles_height as u32);
        let updated = (incremental || canvas.painter_keys.is_some())
            && canvas.buffer_changes.as_ref().is_some_and(|changes| {
                let changed = changes
                    .draws
                    .iter()
                    .cloned()
                    .chain(changes.painter.iter().cloned())
                    .collect::<Vec<_>>();
                bins.update_changed(
                    &canvas.draw_records,
                    &plan.draw_order,
                    canvas.painter_keys.as_deref(),
                    tiles_size,
                    &changed,
                )
            });
        let tile_draw_counts = if updated {
            TileDrawCounts {
                index_count: bins.upload_index_count(),
                chunk_count: bins.active_pages,
            }
        } else if canvas.persistent_root.is_none()
            && canvas.buffer_changes.is_none()
            && canvas.painter_keys.is_none()
        {
            bins.reset_transient(&canvas.draw_records, &plan.draw_order, tiles_size, cursors);
            TileDrawCounts {
                index_count: bins.upload_index_count(),
                chunk_count: bins.active_pages,
            }
        } else {
            build_tile_draw_bins_into(canvas, plan, bins, cursors)
        };
        overrides.coarse_ptcl_capacity =
            Some(bins.coarse_ptcl_capacity(plan, overrides.cached_stack_depths));
        Self::from_scene_with_text_and_tile_draw_counts(
            canvas,
            text,
            tiles_width,
            tiles_height,
            tile_draw_counts,
            overrides,
        )
    }

    fn from_scene_with_text_and_tile_draw_counts(
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        tiles_width: usize,
        tiles_height: usize,
        tile_draw_counts: TileDrawCounts,
        overrides: GpuLengthOverrides,
    ) -> Self {
        let tile_count = tiles_width * tiles_height;
        let width_in_tiles = tiles_width as u32;
        let height_in_tiles = tiles_height as u32;
        let coarse_ptcl_capacity = overrides
            .coarse_ptcl_capacity
            .unwrap_or_else(|| coarse_ptcl_capacity(canvas, width_in_tiles, height_in_tiles));
        let coarse_glyph_capacity = overrides.coarse_glyph_capacity.unwrap_or_else(|| {
            coarse_glyph_capacity(canvas, text, width_in_tiles, height_in_tiles)
        });
        let path_plan_counts = overrides
            .path_plan_counts
            .unwrap_or_else(|| GpuPathPlanCounts {
                scan_chunks: canvas
                    .path_records
                    .iter()
                    .map(|record| record.data_len.div_ceil(SCAN_CHUNK_SIZE) as usize)
                    .sum(),
                cumsum_chunks: canvas
                    .path_records
                    .iter()
                    .map(|record| {
                        let stride = record.tile_x1.saturating_sub(record.tile_x0);
                        let height = record.tile_y1.saturating_sub(record.tile_y0);
                        if stride == 0 {
                            0
                        } else {
                            (height * stride.div_ceil(CUMSUM_CHUNK_SIZE)) as usize
                        }
                    })
                    .sum(),
                cumsum_rows: canvas
                    .path_records
                    .iter()
                    .map(|record| {
                        let stride = record.tile_x1.saturating_sub(record.tile_x0);
                        let height = record.tile_y1.saturating_sub(record.tile_y0);
                        if stride == 0 { 0 } else { height as usize }
                    })
                    .sum(),
            });
        Self {
            line_count: canvas.lines.len(),
            path_count: canvas.path_records.len(),
            draw_count: canvas.draw_records.len(),
            backdrop_record_count: canvas.path_records.len(),
            backdrop_len: canvas.backdrop_pool_capacity as usize,
            segment_capacity: canvas.tile_cnt as usize,
            scan_chunk_count: path_plan_counts.scan_chunks,
            cumsum_chunk_count: path_plan_counts.cumsum_chunks,
            cumsum_row_count: path_plan_counts.cumsum_rows,
            coarse_chunk_count: tile_count.div_ceil(COARSE_CHUNK_SIZE as usize),
            coarse_ptcl_capacity,
            coarse_glyph_capacity,
            tile_draw_index_count: tile_draw_counts.index_count,
            tile_draw_chunk_count: tile_draw_counts.chunk_count,
            text_enabled: text.is_some(),
            text_run_count: canvas.text_runs.len(),
            text_glyph_count: canvas.text_glyphs.len(),
            tiles_width,
            tiles_height,
            tile_count,
            image_pixels: canvas.physical_width() as usize * canvas.physical_height() as usize,
        }
    }
}

/// Per-tile draw references for native coarse binning.
///
/// Coarse used to make every tile scan the whole draw table. These bins keep
/// each tile's candidate draws in canvas order so the GPU only filters local
/// candidates while preserving compositing order.
#[derive(Clone, Debug, Default)]
pub(crate) struct TileDrawBins {
    pub(crate) records: Vec<TileDrawRecord>,
    /// Fixed-size pages: next-page word followed by 256 painter-ordered draw IDs.
    pub(crate) draw_indices: Vec<u32>,
    tile_pages: Vec<Vec<u32>>,
    tile_refs: Vec<Vec<u32>>,
    free_pages: Vec<u32>,
    draw_bboxes: Vec<TileBbox>,
    draw_ranks: Vec<PainterKey>,
    draw_ptcl_capacities: Vec<usize>,
    draw_ptcl_capacity: usize,
    tiles_size: (u32, u32),
    active_pages: usize,
    dirty_records: Vec<usize>,
    dirty_pages: Vec<u32>,
    full_upload: bool,
    flat_full_upload: bool,
    full_upload_records: Vec<TileDrawRecord>,
    full_upload_indices: Vec<u32>,
    page_arena_valid: bool,
    compactions: u64,
    active_batch_marks: Vec<u32>,
    active_batch_generation: u32,
    active_batches: Vec<u32>,
}

impl TileDrawBins {
    pub(crate) fn active_batch_ids(&mut self, tiles: &[u32], draw_batch_ids: &[u32]) -> Vec<u32> {
        self.active_batch_generation = self.active_batch_generation.wrapping_add(1);
        if self.active_batch_generation == 0 {
            self.active_batch_marks.fill(0);
            self.active_batch_generation = 1;
        }
        let generation = self.active_batch_generation;
        self.active_batches.clear();
        for draw in tiles
            .iter()
            .filter_map(|&tile| self.tile_refs.get(tile as usize))
            .flatten()
        {
            let Some(&batch) = draw_batch_ids.get(*draw as usize) else {
                continue;
            };
            if batch == u32::MAX {
                continue;
            }
            let index = batch as usize;
            if index >= self.active_batch_marks.len() {
                self.active_batch_marks.resize(index + 1, 0);
            }
            if self.active_batch_marks[index] != generation {
                self.active_batch_marks[index] = generation;
                self.active_batches.push(batch);
            }
        }
        self.active_batches.sort_unstable();
        self.active_batches.clone()
    }

    #[cfg(test)]
    fn tile_draws(&self, tile: usize) -> Vec<u32> {
        let mut draws = Vec::new();
        self.for_each_tile_draw(tile, |draw| draws.push(draw));
        draws
    }

    fn for_each_tile_draw(&self, tile: usize, mut visit: impl FnMut(u32)) {
        let record = self.records[tile];
        if record.end == 0 {
            return;
        }
        if record.start & TILE_DRAW_FLAT_FLAG != 0 {
            let start = (record.start & !TILE_DRAW_FLAT_FLAG) as usize;
            self.draw_indices[start..start + record.end as usize]
                .iter()
                .copied()
                .for_each(&mut visit);
            return;
        }
        let mut page = record.start;
        let mut remaining = record.end as usize;
        while page != u32::MAX && remaining != 0 {
            let base = page as usize * TILE_DRAW_PAGE_WORDS;
            let count = remaining.min(COARSE_CHUNK_SIZE as usize);
            self.draw_indices[base + 1..base + 1 + count]
                .iter()
                .copied()
                .for_each(&mut visit);
            remaining -= count;
            page = self.draw_indices[base];
        }
    }

    /// Returns painter-ordered draws touching a pixel region without scanning the scene draw
    /// table. Persistent bins already maintain the spatial reverse index; transient bins use the
    /// same uploaded page/flat representation so local offscreen extraction has one code path.
    pub(crate) fn draws_in_bounds(&self, bounds: Bounds, draw_order: &[u32]) -> Vec<u32> {
        let bbox = PixelBounds {
            x0: bounds.x0,
            y0: bounds.y0,
            x1: bounds.x1,
            y1: bounds.y1,
        }
        .tile_bbox(self.tiles_size.0, self.tiles_size.1);
        let mut candidates = HashSet::new();
        for_tile_in_bbox(bbox, self.tiles_size.0, |tile| {
            self.for_each_tile_draw(tile, |draw| {
                candidates.insert(draw);
            });
        });
        if candidates.is_empty() {
            return Vec::new();
        }
        let ranked = candidates.iter().all(|draw| {
            self.draw_ranks
                .get(*draw as usize)
                .is_some_and(|rank| rank.path[0] != u128::MAX)
        });
        if ranked {
            let mut draws = candidates.into_iter().collect::<Vec<_>>();
            draws.sort_unstable_by(|a, b| {
                self.draw_ranks[*a as usize].cmp(&self.draw_ranks[*b as usize])
            });
            draws
        } else {
            draw_order
                .iter()
                .copied()
                .filter(|draw| candidates.contains(draw))
                .collect()
        }
    }

    fn reset(
        &mut self,
        draw_records: &[DrawRecord],
        draw_order: &[u32],
        painter_keys: Option<&[PainterKey]>,
        tiles_size: (u32, u32),
    ) {
        let tile_count = tiles_size.0 as usize * tiles_size.1 as usize;
        if !self.page_arena_valid {
            self.draw_indices.clear();
            self.tile_pages.clear();
            self.free_pages.clear();
            self.active_pages = 0;
            self.page_arena_valid = true;
        }
        self.records.resize(
            tile_count,
            TileDrawRecord {
                start: u32::MAX,
                end: 0,
            },
        );
        self.records.fill(TileDrawRecord {
            start: u32::MAX,
            end: 0,
        });
        if self.tile_pages.len() > tile_count {
            for pages in self.tile_pages.drain(tile_count..) {
                self.active_pages -= pages.len();
                self.free_pages.extend(pages);
            }
        }
        self.tile_pages.resize_with(tile_count, Vec::new);
        self.tile_refs.resize_with(tile_count, Vec::new);
        for draws in &mut self.tile_refs {
            draws.clear();
        }
        self.draw_bboxes.clear();
        self.draw_bboxes.resize(
            draw_records.len(),
            TileBbox {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            },
        );
        self.draw_ranks.clear();
        self.draw_ranks
            .resize(draw_records.len(), PainterKey::inactive());
        self.draw_ptcl_capacities.clear();
        self.draw_ptcl_capacities.resize(draw_records.len(), 0);
        self.draw_ptcl_capacity = 0;
        self.tiles_size = tiles_size;
        self.dirty_records.clear();
        self.dirty_pages.clear();
        let mut stable_order = Vec::new();
        let ordered = if let Some(keys) = painter_keys {
            stable_order.extend(
                keys.iter()
                    .enumerate()
                    .filter_map(|(draw, key)| (key.path[0] != u128::MAX).then_some(draw as u32)),
            );
            stable_order.sort_unstable_by(|a, b| keys[*a as usize].cmp(&keys[*b as usize]));
            stable_order.as_slice()
        } else {
            draw_order
        };
        for (rank, &draw_ix) in ordered.iter().enumerate() {
            self.draw_ranks[draw_ix as usize] = painter_keys.map_or_else(
                || PainterKey {
                    path: std::sync::Arc::from([rank as u128]),
                    local: 0,
                },
                |keys| keys[draw_ix as usize].clone(),
            );
            let bbox = draw_records[draw_ix as usize].tile_bbox(tiles_size.0, tiles_size.1);
            self.draw_bboxes[draw_ix as usize] = bbox;
            let capacity = draw_ptcl_capacity(&draw_records[draw_ix as usize], bbox);
            self.draw_ptcl_capacities[draw_ix as usize] = capacity;
            self.draw_ptcl_capacity += capacity;
            for_tile_in_bbox(bbox, tiles_size.0, |tile| {
                self.tile_refs[tile].push(draw_ix);
            });
        }
        for tile in 0..tile_count {
            self.rewrite_tile(tile);
        }
        self.build_flat_full_upload();
        self.dirty_records.clear();
        self.dirty_pages.clear();
        self.full_upload = true;
    }

    /// Builds page-form tile bins for a one-shot immediate canvas without allocating persistent
    /// per-draw ranks, bboxes, or per-tile vectors. Immediate canvases are rebuilt every frame, so
    /// maintaining mutation indexes only adds CPU and allocation cost without enabling reuse.
    fn reset_transient(
        &mut self,
        draw_records: &[DrawRecord],
        draw_order: &[u32],
        tiles_size: (u32, u32),
        cursors: &mut Vec<u32>,
    ) {
        let tile_count = tiles_size.0 as usize * tiles_size.1 as usize;
        self.records.clear();
        self.records.resize(
            tile_count,
            TileDrawRecord {
                start: u32::MAX,
                end: 0,
            },
        );
        self.draw_ptcl_capacity = 0;
        for &draw in draw_order {
            let record = &draw_records[draw as usize];
            let bbox = record.tile_bbox(tiles_size.0, tiles_size.1);
            self.draw_ptcl_capacity += draw_ptcl_capacity(record, bbox);
            for_tile_in_bbox(bbox, tiles_size.0, |tile| {
                self.records[tile].end += 1;
            });
        }

        let mut next_index = 0u32;
        let mut chunk_count = 0usize;
        for record in &mut self.records {
            let count = record.end;
            record.start = if count == 0 {
                u32::MAX
            } else {
                assert!(next_index < TILE_DRAW_FLAT_FLAG);
                TILE_DRAW_FLAT_FLAG | next_index
            };
            next_index += count;
            chunk_count += count.div_ceil(COARSE_CHUNK_SIZE) as usize;
        }
        self.draw_indices.clear();
        self.draw_indices.resize(next_index as usize, u32::MAX);

        cursors.clear();
        cursors.resize(tile_count, 0);
        for &draw in draw_order {
            let bbox = draw_records[draw as usize].tile_bbox(tiles_size.0, tiles_size.1);
            for_tile_in_bbox(bbox, tiles_size.0, |tile| {
                let ordinal = cursors[tile];
                let start = self.records[tile].start & !TILE_DRAW_FLAT_FLAG;
                self.draw_indices[(start + ordinal) as usize] = draw;
                cursors[tile] += 1;
            });
        }

        self.tile_pages.clear();
        self.tile_refs.clear();
        self.free_pages.clear();
        self.draw_bboxes.clear();
        self.draw_ranks.clear();
        self.draw_ptcl_capacities.clear();
        self.tiles_size = tiles_size;
        self.active_pages = chunk_count;
        self.page_arena_valid = false;
        self.flat_full_upload = false;
        self.full_upload_records.clear();
        self.full_upload_indices.clear();
        self.dirty_records.clear();
        self.dirty_pages.clear();
        self.full_upload = true;
    }

    fn update_changed(
        &mut self,
        draw_records: &[DrawRecord],
        draw_order: &[u32],
        painter_keys: Option<&[PainterKey]>,
        tiles_size: (u32, u32),
        changed: &[std::ops::Range<usize>],
    ) -> bool {
        if self.tiles_size != tiles_size {
            return false;
        }
        if painter_keys.is_none()
            && (self.draw_bboxes.len() != draw_records.len()
                || self.draw_ranks.len() != draw_records.len()
                || self.draw_ptcl_capacities.len() != draw_records.len())
        {
            return false;
        }
        debug_assert!(
            painter_keys.is_some()
                || draw_order
                    .iter()
                    .enumerate()
                    .all(|(rank, &draw)| self.draw_ranks[draw as usize].path[0] == rank as u128)
        );
        let old_len = self.draw_bboxes.len();
        let new_len = draw_records.len();
        let working_len = old_len.max(new_len);
        self.draw_bboxes.resize(
            working_len,
            TileBbox {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            },
        );
        self.draw_ranks.resize(working_len, PainterKey::inactive());
        self.draw_ptcl_capacities.resize(working_len, 0);
        let mut visited_draws = HashSet::new();
        let mut membership_changed = HashSet::new();
        let mut affected_tiles = HashSet::new();
        for draw in changed.iter().flat_map(|range| range.clone()) {
            if draw >= working_len {
                continue;
            }
            if !visited_draws.insert(draw) {
                continue;
            }
            let old_bbox = self.draw_bboxes[draw];
            let old_rank = self.draw_ranks[draw].clone();
            if draw < old_len {
                self.draw_ptcl_capacity -= self.draw_ptcl_capacities[draw];
            }
            if draw < new_len {
                let bbox = draw_records[draw].tile_bbox(tiles_size.0, tiles_size.1);
                self.draw_bboxes[draw] = bbox;
                if let Some(keys) = painter_keys {
                    self.draw_ranks[draw] = keys[draw].clone();
                }
                let capacity = draw_ptcl_capacity(&draw_records[draw], bbox);
                self.draw_ptcl_capacities[draw] = capacity;
                self.draw_ptcl_capacity += capacity;
            } else {
                self.draw_bboxes[draw] = TileBbox {
                    x0: 0,
                    y0: 0,
                    x1: 0,
                    y1: 0,
                };
                self.draw_ranks[draw] = PainterKey::inactive();
                self.draw_ptcl_capacities[draw] = 0;
            }
            if old_bbox != self.draw_bboxes[draw] || old_rank != self.draw_ranks[draw] {
                membership_changed.insert(draw);
                for_tile_in_bbox(old_bbox, tiles_size.0, |tile| {
                    affected_tiles.insert(tile);
                });
                for_tile_in_bbox(self.draw_bboxes[draw], tiles_size.0, |tile| {
                    affected_tiles.insert(tile);
                });
            }
        }
        let page_membership_changed = !affected_tiles.is_empty();
        let mut changed_ids = membership_changed
            .iter()
            .filter_map(|&draw| (draw < new_len).then_some(draw as u32))
            .collect::<Vec<_>>();
        changed_ids.sort_unstable();
        for &tile in &affected_tiles {
            self.tile_refs[tile].retain(|draw| {
                let draw = *draw as usize;
                !membership_changed.contains(&draw)
            });
        }
        for &draw in &changed_ids {
            if self.draw_ranks[draw as usize].path[0] == u128::MAX {
                continue;
            }
            for_tile_in_bbox(self.draw_bboxes[draw as usize], tiles_size.0, |tile| {
                self.tile_refs[tile].push(draw);
            });
        }
        for tile in affected_tiles {
            self.tile_refs[tile].sort_unstable_by(|a, b| {
                self.draw_ranks[*a as usize].cmp(&self.draw_ranks[*b as usize])
            });
            self.rewrite_tile(tile);
        }
        self.draw_bboxes.truncate(new_len);
        self.draw_ranks.truncate(new_len);
        self.draw_ptcl_capacities.truncate(new_len);
        if page_membership_changed && self.flat_full_upload {
            // The GPU currently contains the compact flat representation from the preceding
            // full upload. Keep it for content-only updates; switch once tile membership really
            // changes, uploading the already-maintained persistent page arena exactly once.
            self.flat_full_upload = false;
            self.full_upload_records.clear();
            self.full_upload_indices.clear();
            self.full_upload = true;
        }
        self.maybe_compact_pages();
        true
    }

    fn rewrite_tile(&mut self, tile: usize) {
        let required = self.tile_refs[tile]
            .len()
            .div_ceil(COARSE_CHUNK_SIZE as usize);
        while self.tile_pages[tile].len() < required {
            let page = self.allocate_page();
            self.tile_pages[tile].push(page);
            self.active_pages += 1;
        }
        while self.tile_pages[tile].len() > required {
            let page = self.tile_pages[tile].pop().unwrap();
            self.free_pages.push(page);
            self.active_pages -= 1;
        }
        self.records[tile] = TileDrawRecord {
            start: self.tile_pages[tile].first().copied().unwrap_or(u32::MAX),
            end: self.tile_refs[tile].len() as u32,
        };
        self.dirty_records.push(tile);
        for (local_page, &page) in self.tile_pages[tile].iter().enumerate() {
            let base = page as usize * TILE_DRAW_PAGE_WORDS;
            self.draw_indices[base..base + TILE_DRAW_PAGE_WORDS].fill(u32::MAX);
            self.draw_indices[base] = self.tile_pages[tile]
                .get(local_page + 1)
                .copied()
                .unwrap_or(u32::MAX);
            let start = local_page * COARSE_CHUNK_SIZE as usize;
            let end = (start + COARSE_CHUNK_SIZE as usize).min(self.tile_refs[tile].len());
            self.draw_indices[base + 1..base + 1 + end - start]
                .copy_from_slice(&self.tile_refs[tile][start..end]);
            self.dirty_pages.push(page);
        }
    }

    fn build_flat_full_upload(&mut self) {
        self.flat_full_upload = true;
        self.full_upload_records.clear();
        self.full_upload_records.resize(
            self.tile_refs.len(),
            TileDrawRecord {
                start: u32::MAX,
                end: 0,
            },
        );
        self.full_upload_indices.clear();
        for (tile, draws) in self.tile_refs.iter().enumerate() {
            if draws.is_empty() {
                continue;
            }
            let start = self.full_upload_indices.len();
            assert!(start < TILE_DRAW_FLAT_FLAG as usize);
            self.full_upload_records[tile] = TileDrawRecord {
                start: TILE_DRAW_FLAT_FLAG | start as u32,
                end: draws.len() as u32,
            };
            self.full_upload_indices.extend_from_slice(draws);
        }
    }

    fn allocate_page(&mut self) -> u32 {
        if let Some(page) = self.free_pages.pop() {
            return page;
        }
        let page = (self.draw_indices.len() / TILE_DRAW_PAGE_WORDS) as u32;
        self.draw_indices
            .resize(self.draw_indices.len() + TILE_DRAW_PAGE_WORDS, u32::MAX);
        page
    }

    fn maybe_compact_pages(&mut self) {
        let total = self.draw_indices.len() / TILE_DRAW_PAGE_WORDS;
        if total < 16 || self.free_pages.len() * 10 <= total * 3 {
            return;
        }
        self.draw_indices.clear();
        self.free_pages.clear();
        self.active_pages = 0;
        for pages in &mut self.tile_pages {
            pages.clear();
        }
        for tile in 0..self.tile_refs.len() {
            self.rewrite_tile(tile);
        }
        self.full_upload = true;
        self.flat_full_upload = false;
        self.full_upload_records.clear();
        self.full_upload_indices.clear();
        self.compactions += 1;
    }

    pub(crate) fn take_dirty(&mut self) -> (bool, Vec<usize>, Vec<u32>) {
        let full = std::mem::take(&mut self.full_upload);
        self.dirty_records.sort_unstable();
        self.dirty_records.dedup();
        self.dirty_pages.sort_unstable();
        self.dirty_pages.dedup();
        (
            full,
            std::mem::take(&mut self.dirty_records),
            std::mem::take(&mut self.dirty_pages),
        )
    }

    pub(crate) fn active_page_count(&self) -> usize {
        self.active_pages
    }

    pub(crate) fn upload_records(&self) -> &[TileDrawRecord] {
        if self.flat_full_upload {
            &self.full_upload_records
        } else {
            &self.records
        }
    }

    pub(crate) fn upload_indices(&self) -> &[u32] {
        if self.flat_full_upload {
            &self.full_upload_indices
        } else {
            &self.draw_indices
        }
    }

    pub(crate) fn upload_index_count(&self) -> usize {
        self.upload_indices().len()
    }

    pub(crate) fn compactions(&self) -> u64 {
        self.compactions
    }

    fn coarse_ptcl_capacity(
        &self,
        plan: &ExecPlan,
        cached_stack_depths: Option<(usize, usize)>,
    ) -> usize {
        let (clip_depth, group_depth) =
            cached_stack_depths.unwrap_or_else(|| plan_stack_depths(plan));
        // Every non-empty tile needs one terminator. Fused layer-stack entries additionally emit
        // one begin and one end particle per active tile. These hidden wrappers are absent from
        // draw_order/tile bins, so omitting them underallocates coarse work and lets later tiles
        // overwrite the page arena (portable backends exposed this as a missing second tile).
        self.records.len()
            + self.draw_ptcl_capacity
            + self.records.len() * 2 * (clip_depth + group_depth)
    }
}

fn draw_ptcl_capacity(draw: &DrawRecord, bbox: TileBbox) -> usize {
    let tiles = bbox.tile_count() as usize;
    let draw_particle =
        ((draw.has_path() || draw.has_analytic_geometry() || draw.glyph_run_id().is_some())
            && matches!(
                draw.tag(),
                DrawTag::Brush | DrawTag::PathGlyph | DrawTag::Clip
            )) as usize;
    let group_begin = (draw.has_path()
        && matches!(
            draw.tag(),
            DrawTag::Opacity | DrawTag::Blend | DrawTag::Isolate
        )) as usize;
    let layer_end = ((matches!(draw.tag(), DrawTag::Clip)
        && (draw.has_path() || draw.sdf_range().is_some()))
        || (draw.has_path()
            && matches!(
                draw.tag(),
                DrawTag::Opacity | DrawTag::Blend | DrawTag::Isolate
            ))) as usize;
    tiles * (draw_particle + group_begin + layer_end)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TileDrawCounts {
    pub(crate) index_count: usize,
    pub(crate) chunk_count: usize,
}

#[cfg(test)]
pub(crate) fn build_tile_draw_bins(canvas: &Canvas) -> TileDrawBins {
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    build_tile_draw_bins_into(canvas, &plan, &mut bins, &mut cursors);
    bins
}

pub(crate) fn build_tile_draw_bins_into(
    canvas: &Canvas,
    plan: &ExecPlan,
    bins: &mut TileDrawBins,
    cursors: &mut Vec<u32>,
) -> TileDrawCounts {
    build_tile_draw_bins_for_draws_into(
        &canvas.draw_records,
        &plan.draw_order,
        canvas.painter_keys.as_deref(),
        (canvas.width_in_tiles(), canvas.height_in_tiles()),
        bins,
        cursors,
    )
}

pub(crate) fn build_tile_draw_bins_for_draws_into(
    draw_records: &[DrawRecord],
    draw_order: &[u32],
    painter_keys: Option<&[PainterKey]>,
    tiles_size: (u32, u32),
    bins: &mut TileDrawBins,
    cursors: &mut Vec<u32>,
) -> TileDrawCounts {
    bins.reset(draw_records, draw_order, painter_keys, tiles_size);
    cursors.clear();

    TileDrawCounts {
        index_count: bins.upload_index_count(),
        chunk_count: bins.active_pages,
    }
}

fn for_tile_in_bbox(mut bbox: TileBbox, width_in_tiles: u32, mut visit: impl FnMut(usize)) {
    bbox.x1 = bbox.x1.min(width_in_tiles);
    if bbox.x0 >= bbox.x1 {
        return;
    }
    for tile_y in bbox.y0..bbox.y1 {
        let row_start = tile_y * width_in_tiles;
        for tile_x in bbox.x0..bbox.x1 {
            visit((row_start + tile_x) as usize);
        }
    }
}

fn coarse_glyph_capacity(
    canvas: &Canvas,
    text: Option<&PreparedTextData>,
    width_in_tiles: u32,
    height_in_tiles: u32,
) -> usize {
    let Some(text) = text else {
        return 0;
    };

    canvas
        .draw_records
        .iter()
        .filter(|draw| matches!(draw.tag(), DrawTag::Brush))
        .filter_map(|draw| draw.glyph_run_id().map(|run_id| (draw, run_id)))
        .map(|(draw, run_id)| {
            let draw_bbox = draw.tile_bbox(width_in_tiles, height_in_tiles);
            text.run_glyph_indices(run_id)
                .filter_map(|glyph_id| {
                    let glyph_bbox = bounds_tile_bbox(
                        text.glyph_bounds(glyph_id)?,
                        width_in_tiles,
                        height_in_tiles,
                    );
                    Some(tile_bbox_intersection_count(draw_bbox, glyph_bbox))
                })
                .sum::<usize>()
        })
        .sum()
}

pub(crate) fn coarse_glyph_capacity_for_draw(
    canvas: &Canvas,
    text: Option<&PreparedTextData>,
    draw_id: usize,
) -> usize {
    let Some(text) = text else {
        return 0;
    };
    let Some(draw) = canvas.draw_records.get(draw_id) else {
        return 0;
    };
    if !matches!(draw.tag(), DrawTag::Brush) {
        return 0;
    }
    let Some(run_id) = draw.glyph_run_id() else {
        return 0;
    };
    let width = canvas.width_in_tiles();
    let height = canvas.height_in_tiles();
    let draw_bbox = draw.tile_bbox(width, height);
    text.run_glyph_indices(run_id)
        .filter_map(|glyph_id| {
            let glyph_bbox = bounds_tile_bbox(text.glyph_bounds(glyph_id)?, width, height);
            Some(tile_bbox_intersection_count(draw_bbox, glyph_bbox))
        })
        .sum()
}

#[cfg(test)]
fn tile_draw_counts_for_order(
    draw_records: &[DrawRecord],
    draw_order: &[u32],
    tiles_size: (u32, u32),
) -> TileDrawCounts {
    let (width_in_tiles, height_in_tiles) = tiles_size;
    let tile_count = width_in_tiles as usize * height_in_tiles as usize;
    let mut counts = vec![0usize; tile_count];
    for &draw_ix in draw_order {
        let draw = &draw_records[draw_ix as usize];
        for_tile_in_bbox(
            draw.tile_bbox(width_in_tiles, height_in_tiles),
            width_in_tiles,
            |tile_ix| {
                counts[tile_ix] += 1;
            },
        );
    }
    let chunk_count: usize = counts
        .into_iter()
        .map(|count| count.div_ceil(COARSE_CHUNK_SIZE as usize))
        .sum();
    TileDrawCounts {
        index_count: chunk_count * TILE_DRAW_PAGE_WORDS,
        chunk_count,
    }
}

fn bounds_tile_bbox(bounds: Bounds, width_in_tiles: u32, height_in_tiles: u32) -> TileBbox {
    PixelBounds {
        x0: bounds.x0,
        y0: bounds.y0,
        x1: bounds.x1,
        y1: bounds.y1,
    }
    .tile_bbox(width_in_tiles, height_in_tiles)
}

fn tile_bbox_intersection_count(a: TileBbox, b: TileBbox) -> usize {
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    x1.saturating_sub(x0) as usize * y1.saturating_sub(y0) as usize
}

fn coarse_ptcl_capacity(canvas: &Canvas, width_in_tiles: u32, height_in_tiles: u32) -> usize {
    let draw_particles = canvas
        .draw_records
        .iter()
        .filter(|draw| {
            (draw.has_path() || draw.has_analytic_geometry() || draw.glyph_run_id().is_some())
                && matches!(
                    draw.tag(),
                    DrawTag::Brush | DrawTag::PathGlyph | DrawTag::Clip
                )
        })
        .map(|draw| draw.tile_bbox(width_in_tiles, height_in_tiles).tile_count() as usize)
        .sum::<usize>();
    let group_begin_particles = canvas
        .draw_records
        .iter()
        .filter(|draw| {
            draw.has_path()
                && matches!(
                    draw.tag(),
                    DrawTag::Opacity | DrawTag::Blend | DrawTag::Isolate
                )
        })
        .map(|draw| draw.tile_bbox(width_in_tiles, height_in_tiles).tile_count() as usize)
        .sum::<usize>();
    let layer_end_particles = canvas
        .draw_records
        .iter()
        .filter(|draw| {
            let clip_needs_end = matches!(draw.tag(), DrawTag::Clip)
                && (draw.has_path() || draw.sdf_range().is_some());
            let path_group_needs_end = draw.has_path()
                && matches!(
                    draw.tag(),
                    DrawTag::Opacity | DrawTag::Blend | DrawTag::Isolate
                );
            clip_needs_end || path_group_needs_end
        })
        .map(|draw| draw.tile_bbox(width_in_tiles, height_in_tiles).tile_count() as usize)
        .sum::<usize>();
    width_in_tiles as usize * height_in_tiles as usize
        + draw_particles
        + group_begin_particles
        + layer_end_particles
}

pub(crate) fn plan_stack_depths(plan: &ExecPlan) -> (usize, usize) {
    plan_stack_depths_for_ops(&plan.ops, plan)
}

fn plan_stack_depths_for_ops(ops: &[ExecOp], plan: &ExecPlan) -> (usize, usize) {
    let mut max_clip_depth = 0;
    let mut max_group_depth = 0;
    for op in ops {
        match op {
            ExecOp::DrawBatch { layer_stack, .. } => {
                let (clip_depth, group_depth) =
                    layer_stack_depths(&plan.layer_stack_data[layer_stack.clone()]);
                max_clip_depth = max_clip_depth.max(clip_depth);
                max_group_depth = max_group_depth.max(group_depth);
            }
            ExecOp::OffscreenLayer {
                outer_stack,
                children,
                ..
            } => {
                let (clip_depth, group_depth) =
                    layer_stack_depths(&plan.layer_stack_data[outer_stack.clone()]);
                let (child_clip_depth, child_group_depth) =
                    plan_stack_depths_for_ops(children, plan);
                max_clip_depth = max_clip_depth.max(clip_depth).max(child_clip_depth);
                max_group_depth = max_group_depth.max(group_depth).max(child_group_depth);
            }
            ExecOp::OffscreenMaskLayer {
                outer_stack,
                content,
                mask,
                ..
            } => {
                let (clip_depth, group_depth) =
                    layer_stack_depths(&plan.layer_stack_data[outer_stack.clone()]);
                let (content_clip_depth, content_group_depth) =
                    plan_stack_depths_for_ops(content, plan);
                let (mask_clip_depth, mask_group_depth) = plan_stack_depths_for_ops(mask, plan);
                max_clip_depth = max_clip_depth
                    .max(clip_depth)
                    .max(content_clip_depth)
                    .max(mask_clip_depth);
                max_group_depth = max_group_depth
                    .max(group_depth)
                    .max(content_group_depth)
                    .max(mask_group_depth);
            }
            _ => {}
        }
    }
    (max_clip_depth, max_group_depth)
}

fn layer_stack_depths(entries: &[LayerStackEntry]) -> (usize, usize) {
    (
        entries
            .iter()
            .filter(|entry| matches!(entry, LayerStackEntry::Clip { .. }))
            .count(),
        entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    LayerStackEntry::Opacity { .. } | LayerStackEntry::Blend { .. }
                )
            })
            .count(),
    )
}

pub(crate) fn required_scratch_count(plan: &ExecPlan) -> usize {
    max_scratch_for_ops(&plan.ops, 0)
}

fn max_scratch_for_ops(ops: &[ExecOp], held: usize) -> usize {
    let mut max_count = held;
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                retained_id,
                layer,
                outer_stack,
                children,
                ..
            } => match layer {
                Layer::Isolate | Layer::Opacity(_) | Layer::Blend(_) => {
                    let source_held = held + 1;
                    max_count = max_count.max(source_held + 1);
                    max_count = max_count.max(max_scratch_for_ops(children, source_held));
                }
                Layer::Filter { filter, .. } => {
                    let source_held = held + 1;
                    max_count = max_count.max(source_held + filter_scratch_extra(filter));
                    if !outer_stack.is_empty() {
                        max_count = max_count.max(source_held);
                    }
                    max_count = max_count.max(max_scratch_for_ops(children, source_held));
                }
                Layer::Backdrop { filter, .. } => {
                    // Retained backdrops keep both filtered output and the
                    // painter-order source history live while rerendering.
                    let backdrop_held = held + 1 + usize::from(retained_id.is_some());
                    max_count = max_count.max(backdrop_held + filter_scratch_extra(filter));
                    max_count = max_count.max(backdrop_held + 1);
                    let content_held = held + 1;
                    max_count = max_count.max(max_scratch_for_ops(children, content_held));
                }
                _ => {
                    max_count = max_count.max(max_scratch_for_ops(children, held));
                }
            },
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                let content_held = held + 1;
                max_count = max_count.max(max_scratch_for_ops(content, content_held));
                let mask_source_held = held + 2;
                max_count = max_count.max(mask_source_held + 1);
                max_count = max_count.max(max_scratch_for_ops(mask, mask_source_held));
            }
            _ => {}
        }
    }
    max_count
}

pub(crate) fn filter_scratch_extra(filter: &Filter) -> usize {
    match filter {
        Filter::Chain { filters, .. } => {
            filters.iter().map(filter_scratch_extra).max().unwrap_or(0)
        }
        Filter::Graph { primitives, .. } => graph_scratch_extra(primitives),
        Filter::RectLiquidGlass(glass) => 2 + usize::from(glass.blur_radius > 0),
        Filter::Blur {
            std_dev_x,
            std_dev_y,
            sampling,
        } => {
            usize::from(std_dev_x.max(*std_dev_y) > 0.0)
                + usize::from(sampling.factor() > 1 && std_dev_x.max(*std_dev_y) > 0.0)
        }
        Filter::ConvolveMatrix(_) => 1,
        Filter::DiffuseLighting(_) => 1,
        Filter::SpecularLighting(_) => 1,
        Filter::Offset { .. } => 1,
        Filter::Morphology { .. } => 2,
        Filter::DropShadow { std_dev, .. } => 1 + usize::from(std_dev.max(0.0) > 0.0),
        _ => 0,
    }
}

fn graph_scratch_extra(primitives: &[FilterPrimitive]) -> usize {
    let source_alpha = primitives.iter().any(|primitive| {
        primitive.input == FilterInput::SourceAlpha
            || primitive.input2 == Some(FilterInput::SourceAlpha)
    });
    let unary_temp = primitives
        .iter()
        .filter_map(|primitive| match &primitive.kind {
            FilterPrimitiveKind::Filter(filter) => Some(1 + filter_scratch_extra(filter)),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    primitives.len() + usize::from(source_alpha) + unary_temp
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct GpuCanvasConfig {
    pub width: u32,
    pub height: u32,
    pub tiles_width: u32,
    pub tiles_height: u32,
    pub line_count: u32,
    pub path_count: u32,
    pub draw_count: u32,
    pub backdrop_record_count: u32,
    pub backdrop_len: u32,
    pub segment_capacity: u32,
    pub scan_chunk_count: u32,
    pub cumsum_chunk_count: u32,
    pub cumsum_row_count: u32,
    pub coarse_chunk_count: u32,
    pub coarse_ptcl_capacity: u32,
    pub clear_color: u32,
}

impl GpuCanvasConfig {
    pub(crate) fn new(canvas: &Canvas, lengths: GpuBufferLengths, clear_color: u32) -> Self {
        Self {
            width: canvas.physical_width(),
            height: canvas.physical_height(),
            tiles_width: canvas.width_in_tiles(),
            tiles_height: canvas.height_in_tiles(),
            line_count: lengths.line_count as u32,
            path_count: lengths.path_count as u32,
            draw_count: lengths.draw_count as u32,
            backdrop_record_count: lengths.backdrop_record_count as u32,
            backdrop_len: lengths.backdrop_len as u32,
            segment_capacity: lengths.segment_capacity as u32,
            scan_chunk_count: lengths.scan_chunk_count as u32,
            cumsum_chunk_count: lengths.cumsum_chunk_count as u32,
            cumsum_row_count: lengths.cumsum_row_count as u32,
            coarse_chunk_count: lengths.coarse_chunk_count as u32,
            coarse_ptcl_capacity: lengths.coarse_ptcl_capacity as u32,
            clear_color,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq, Eq)]
pub(crate) struct GpuScanChunk {
    pub path_id: u32,
    pub backdrop_offset: u32,
    pub segment_start: u32,
    pub len: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq, Eq)]
pub(crate) struct GpuScanChunkRange {
    pub start: u32,
    pub end: u32,
}

#[cfg(test)]
#[cfg(test)]
pub(crate) fn build_scan_chunks(canvas: &Canvas) -> (Vec<GpuScanChunk>, Vec<GpuScanChunkRange>) {
    let lengths = GpuBufferLengths::from_scene(canvas);
    let mut chunks = Vec::with_capacity(lengths.scan_chunk_count);
    let mut ranges = Vec::with_capacity(canvas.path_records.len());
    build_scan_chunks_into(canvas, lengths.scan_chunk_count, &mut chunks, &mut ranges);
    (chunks, ranges)
}

#[cfg(test)]
pub(crate) fn build_scan_chunks_into(
    canvas: &Canvas,
    scan_chunk_count: usize,
    chunks: &mut Vec<GpuScanChunk>,
    ranges: &mut Vec<GpuScanChunkRange>,
) {
    chunks.clear();
    chunks.reserve(scan_chunk_count);
    ranges.clear();
    ranges.resize(canvas.path_records.len(), GpuScanChunkRange::default());

    for record in &canvas.path_records {
        let range_start = chunks.len() as u32;
        let mut local = 0;
        while local < record.data_len {
            let len = (record.data_len - local).min(SCAN_CHUNK_SIZE);
            chunks.push(GpuScanChunk {
                path_id: record.path_id,
                backdrop_offset: record.data_offset + local,
                segment_start: record.segment_start,
                len,
            });
            local += len;
        }
        if let Some(range) = ranges.get_mut(record.path_id as usize) {
            *range = GpuScanChunkRange {
                start: range_start,
                end: chunks.len() as u32,
            };
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GpuCumsumPlan {
    pub chunk_backdrop_offsets: Vec<u32>,
    pub chunk_lens: Vec<u32>,
    pub row_chunk_starts: Vec<u32>,
    pub row_chunk_ends: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GpuCumsumChunk {
    backdrop_offset: u32,
    len: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GpuCumsumRow {
    chunk_start: u32,
    chunk_end: u32,
}

#[derive(Clone, Copy, Debug)]
struct PathPlanAllocation {
    scan: ArenaAllocation,
    cumsum_chunks: ArenaAllocation,
    cumsum_rows: ArenaAllocation,
}

#[derive(Debug, Default)]
pub(crate) struct GpuPathPlanDirty {
    pub(crate) scan_chunks: Vec<Range<usize>>,
    pub(crate) scan_ranges: Vec<Range<usize>>,
    pub(crate) cumsum_chunks: Vec<Range<usize>>,
    pub(crate) cumsum_rows: Vec<Range<usize>>,
}

/// Stable scan/cumsum allocations keyed by physical path slot.
///
/// Retained path records already use stable arena offsets. Keeping their derived GPU plans in
/// matching variable-sized allocations avoids rebuilding and diffing every path when one chunk
/// changes shape. Vacant arena slots compile to zero-length work and are therefore safe to leave
/// in the dispatch address space until a normal fragmentation-triggered compaction.
pub(crate) struct PersistentPathPlans {
    scan_chunks: SceneArena<GpuScanChunk>,
    scan_ranges: Vec<GpuScanChunkRange>,
    cumsum_chunks: SceneArena<GpuCumsumChunk>,
    cumsum_rows: SceneArena<GpuCumsumRow>,
    cumsum_plan: GpuCumsumPlan,
    allocations: Vec<Option<PathPlanAllocation>>,
    row_chunk_counts: Vec<Option<Vec<u32>>>,
    initialized: bool,
    dirty_scan_chunks: Vec<Range<usize>>,
    dirty_scan_ranges: Vec<Range<usize>>,
    dirty_cumsum_chunks: Vec<Range<usize>>,
    dirty_cumsum_rows: Vec<Range<usize>>,
}

impl Default for PersistentPathPlans {
    fn default() -> Self {
        Self {
            scan_chunks: SceneArena::new(GpuScanChunk::default()),
            scan_ranges: Vec::new(),
            cumsum_chunks: SceneArena::new(GpuCumsumChunk::default()),
            cumsum_rows: SceneArena::new(GpuCumsumRow::default()),
            cumsum_plan: GpuCumsumPlan::default(),
            allocations: Vec::new(),
            row_chunk_counts: Vec::new(),
            initialized: false,
            dirty_scan_chunks: Vec::new(),
            dirty_scan_ranges: Vec::new(),
            dirty_cumsum_chunks: Vec::new(),
            dirty_cumsum_rows: Vec::new(),
        }
    }
}

impl PersistentPathPlans {
    pub(crate) fn update(
        &mut self,
        canvas: &Canvas,
        changed_ranges: Option<&[Range<usize>]>,
    ) -> GpuPathPlanCounts {
        let path_len = canvas.path_records.len();
        let old_len = self.allocations.len();
        let full = !self.initialized || changed_ranges.is_none();

        let scan_compactions = self.scan_chunks.compactions();
        let cumsum_compactions = self.cumsum_chunks.compactions();
        if full {
            *self = Self::default();
            self.allocations.resize(path_len, None);
            self.row_chunk_counts.resize(path_len, None);
            self.scan_ranges
                .resize(path_len, GpuScanChunkRange::default());
            if path_len != 0 {
                self.dirty_scan_ranges.push(0..path_len);
            }
        } else {
            let removed = self
                .allocations
                .drain(path_len.min(old_len)..)
                .flatten()
                .collect::<Vec<_>>();
            for allocation in removed {
                self.remove_allocation(allocation);
            }
            self.allocations.resize(path_len, None);
            self.row_chunk_counts.truncate(path_len);
            self.row_chunk_counts.resize(path_len, None);
            self.scan_ranges
                .resize(path_len, GpuScanChunkRange::default());
        }

        if full {
            for (path_id, record) in canvas.path_records.iter().enumerate() {
                self.update_path(path_id, record);
            }
        } else {
            let mut changed = Vec::new();
            for range in changed_ranges.unwrap() {
                let start = range.start.min(path_len);
                let end = range.end.min(path_len);
                changed.extend(start..end);
            }
            if path_len > old_len {
                changed.extend(old_len..path_len);
            }
            changed.sort_unstable();
            changed.dedup();
            for path_id in changed {
                self.update_path(path_id, &canvas.path_records[path_id]);
            }
        }

        if self.scan_chunks.compactions() != scan_compactions {
            self.refresh_all_scan_ranges();
        }
        if self.cumsum_chunks.compactions() != cumsum_compactions {
            self.refresh_all_cumsum_rows();
        }
        self.initialized = true;
        self.sync_dirty_outputs();
        self.counts()
    }

    fn update_path(&mut self, path_id: usize, record: &PathRecord) {
        // A vacant PathRecord is all zeroes. A live record always owns its physical path slot;
        // checking both properties prevents an arena hole from masquerading as path zero.
        let live = record.path_id as usize == path_id
            && (record.line_count != 0 || record.data_len != 0 || record.segment_capacity != 0);
        if !live {
            if let Some(allocation) = self.allocations[path_id].take() {
                self.remove_allocation(allocation);
            }
            self.row_chunk_counts[path_id] = None;
            self.set_scan_range(path_id, GpuScanChunkRange::default());
            return;
        }

        let scan = scan_chunks_for_record(record);
        let (cumsum_chunks, row_chunk_counts) = cumsum_for_record(record);
        let allocation = if let Some(allocation) = self.allocations[path_id] {
            self.scan_chunks.replace(allocation.scan, &scan);
            self.cumsum_chunks
                .replace(allocation.cumsum_chunks, &cumsum_chunks);
            allocation
        } else {
            PathPlanAllocation {
                scan: self.scan_chunks.insert(&scan),
                cumsum_chunks: self.cumsum_chunks.insert(&cumsum_chunks),
                cumsum_rows: self.cumsum_rows.insert(&[]),
            }
        };
        self.allocations[path_id] = Some(allocation);
        self.row_chunk_counts[path_id] = Some(row_chunk_counts.clone());
        self.refresh_scan_range(path_id);
        self.refresh_cumsum_rows(path_id, &row_chunk_counts);
    }

    fn remove_allocation(&mut self, allocation: PathPlanAllocation) {
        self.scan_chunks.remove(allocation.scan);
        self.cumsum_chunks.remove(allocation.cumsum_chunks);
        self.cumsum_rows.remove(allocation.cumsum_rows);
    }

    fn refresh_scan_range(&mut self, path_id: usize) {
        let Some(allocation) = self.allocations[path_id] else {
            self.set_scan_range(path_id, GpuScanChunkRange::default());
            return;
        };
        let range = self.scan_chunks.range(allocation.scan);
        self.set_scan_range(
            path_id,
            GpuScanChunkRange {
                start: range.start as u32,
                end: range.end as u32,
            },
        );
    }

    fn set_scan_range(&mut self, path_id: usize, range: GpuScanChunkRange) {
        if self.scan_ranges[path_id] != range {
            self.scan_ranges[path_id] = range;
            merge_range(&mut self.dirty_scan_ranges, path_id..path_id + 1);
        }
    }

    fn refresh_all_scan_ranges(&mut self) {
        for path_id in 0..self.allocations.len() {
            self.refresh_scan_range(path_id);
        }
    }

    fn refresh_cumsum_rows(&mut self, path_id: usize, row_chunk_counts: &[u32]) {
        let allocation = self.allocations[path_id].unwrap();
        let mut chunk = self.cumsum_chunks.range(allocation.cumsum_chunks).start as u32;
        let rows = row_chunk_counts
            .iter()
            .map(|&count| {
                let row = GpuCumsumRow {
                    chunk_start: chunk,
                    chunk_end: chunk + count,
                };
                chunk += count;
                row
            })
            .collect::<Vec<_>>();
        self.cumsum_rows.replace(allocation.cumsum_rows, &rows);
    }

    fn refresh_all_cumsum_rows(&mut self) {
        for path_id in 0..self.allocations.len() {
            let Some(allocation) = self.allocations[path_id] else {
                continue;
            };
            let record_chunks = self.cumsum_chunks.range(allocation.cumsum_chunks);
            let counts = self.row_chunk_counts[path_id]
                .as_deref()
                .expect("live path plan has cumsum row metadata");
            let mut chunk = record_chunks.start as u32;
            let rows = counts
                .iter()
                .map(|&count| {
                    let row = GpuCumsumRow {
                        chunk_start: chunk,
                        chunk_end: chunk + count,
                    };
                    chunk += count;
                    row
                })
                .collect::<Vec<_>>();
            self.cumsum_rows.replace(allocation.cumsum_rows, &rows);
        }
    }

    fn sync_dirty_outputs(&mut self) {
        merge_ranges(
            &mut self.dirty_scan_chunks,
            self.scan_chunks.take_dirty_ranges(),
        );
        let chunk_ranges = self.cumsum_chunks.take_dirty_ranges();
        self.cumsum_plan
            .chunk_backdrop_offsets
            .resize(self.cumsum_chunks.values().len(), 0);
        self.cumsum_plan
            .chunk_lens
            .resize(self.cumsum_chunks.values().len(), 0);
        for range in &chunk_ranges {
            for (index, chunk) in self.cumsum_chunks.values()[range.clone()]
                .iter()
                .enumerate()
            {
                let index = range.start + index;
                self.cumsum_plan.chunk_backdrop_offsets[index] = chunk.backdrop_offset;
                self.cumsum_plan.chunk_lens[index] = chunk.len;
            }
        }
        merge_ranges(&mut self.dirty_cumsum_chunks, chunk_ranges);

        let row_ranges = self.cumsum_rows.take_dirty_ranges();
        self.cumsum_plan
            .row_chunk_starts
            .resize(self.cumsum_rows.values().len(), 0);
        self.cumsum_plan
            .row_chunk_ends
            .resize(self.cumsum_rows.values().len(), 0);
        for range in &row_ranges {
            for (index, row) in self.cumsum_rows.values()[range.clone()].iter().enumerate() {
                let index = range.start + index;
                self.cumsum_plan.row_chunk_starts[index] = row.chunk_start;
                self.cumsum_plan.row_chunk_ends[index] = row.chunk_end;
            }
        }
        merge_ranges(&mut self.dirty_cumsum_rows, row_ranges);
    }

    pub(crate) fn counts(&self) -> GpuPathPlanCounts {
        GpuPathPlanCounts {
            scan_chunks: self.scan_chunks.values().len(),
            cumsum_chunks: self.cumsum_chunks.values().len(),
            cumsum_rows: self.cumsum_rows.values().len(),
        }
    }

    pub(crate) fn scan_chunks(&self) -> &[GpuScanChunk] {
        self.scan_chunks.values()
    }

    pub(crate) fn scan_ranges(&self) -> &[GpuScanChunkRange] {
        &self.scan_ranges
    }

    pub(crate) fn cumsum_plan(&self) -> &GpuCumsumPlan {
        &self.cumsum_plan
    }

    pub(crate) fn take_dirty(&mut self) -> GpuPathPlanDirty {
        GpuPathPlanDirty {
            scan_chunks: std::mem::take(&mut self.dirty_scan_chunks),
            scan_ranges: std::mem::take(&mut self.dirty_scan_ranges),
            cumsum_chunks: std::mem::take(&mut self.dirty_cumsum_chunks),
            cumsum_rows: std::mem::take(&mut self.dirty_cumsum_rows),
        }
    }
}

fn scan_chunks_for_record(record: &PathRecord) -> Vec<GpuScanChunk> {
    let mut chunks = Vec::with_capacity(record.data_len.div_ceil(SCAN_CHUNK_SIZE) as usize);
    let mut local = 0;
    while local < record.data_len {
        let len = (record.data_len - local).min(SCAN_CHUNK_SIZE);
        chunks.push(GpuScanChunk {
            path_id: record.path_id,
            backdrop_offset: record.data_offset + local,
            segment_start: record.segment_start,
            len,
        });
        local += len;
    }
    chunks
}

fn cumsum_for_record(record: &PathRecord) -> (Vec<GpuCumsumChunk>, Vec<u32>) {
    let stride = record.tile_x1.saturating_sub(record.tile_x0);
    let height = record.tile_y1.saturating_sub(record.tile_y0);
    let mut chunks = Vec::new();
    let mut rows = Vec::new();
    if stride == 0 || height == 0 {
        return (chunks, rows);
    }
    chunks.reserve((height * stride.div_ceil(CUMSUM_CHUNK_SIZE)) as usize);
    rows.reserve(height as usize);
    for row in 0..height {
        let before = chunks.len();
        let row_offset = record.data_offset + row * stride;
        let mut local_x = 0;
        while local_x < stride {
            let len = (stride - local_x).min(CUMSUM_CHUNK_SIZE);
            chunks.push(GpuCumsumChunk {
                backdrop_offset: row_offset + local_x,
                len,
            });
            local_x += len;
        }
        rows.push((chunks.len() - before) as u32);
    }
    (chunks, rows)
}

fn merge_ranges(target: &mut Vec<Range<usize>>, ranges: Vec<Range<usize>>) {
    for range in ranges {
        merge_range(target, range);
    }
}

fn merge_range(target: &mut Vec<Range<usize>>, mut range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    let mut index = 0;
    while index < target.len() {
        if target[index].end < range.start || range.end < target[index].start {
            index += 1;
            continue;
        }
        let current = target.swap_remove(index);
        range.start = range.start.min(current.start);
        range.end = range.end.max(current.end);
    }
    target.push(range);
    target.sort_unstable_by_key(|range| range.start);
}

#[cfg(test)]
#[cfg(test)]
pub(crate) fn build_cumsum_plan(canvas: &Canvas) -> GpuCumsumPlan {
    let lengths = GpuBufferLengths::from_scene(canvas);
    let mut plan = GpuCumsumPlan::default();
    build_cumsum_plan_into(canvas, lengths, &mut plan);
    plan
}

#[cfg(test)]
pub(crate) fn build_cumsum_plan_into(
    canvas: &Canvas,
    lengths: GpuBufferLengths,
    plan: &mut GpuCumsumPlan,
) {
    plan.chunk_backdrop_offsets.clear();
    plan.chunk_lens.clear();
    plan.row_chunk_starts.clear();
    plan.row_chunk_ends.clear();
    plan.chunk_backdrop_offsets
        .reserve(lengths.cumsum_chunk_count);
    plan.chunk_lens.reserve(lengths.cumsum_chunk_count);
    plan.row_chunk_starts.reserve(lengths.cumsum_row_count);
    plan.row_chunk_ends.reserve(lengths.cumsum_row_count);

    for record in &canvas.path_records {
        let stride = record.tile_x1.saturating_sub(record.tile_x0);
        let height = record.tile_y1.saturating_sub(record.tile_y0);
        if stride == 0 || height == 0 {
            continue;
        }

        for row in 0..height {
            let row_start = plan.chunk_backdrop_offsets.len() as u32;
            let row_offset = record.data_offset + row * stride;
            let mut local_x = 0;
            while local_x < stride {
                let len = (stride - local_x).min(CUMSUM_CHUNK_SIZE);
                plan.chunk_backdrop_offsets.push(row_offset + local_x);
                plan.chunk_lens.push(len);
                local_x += len;
            }
            plan.row_chunk_starts.push(row_start);
            plan.row_chunk_ends
                .push(plan.chunk_backdrop_offsets.len() as u32);
        }
    }
}

#[cfg(test)]
mod text_length_tests {
    use peniko::{Color, kurbo::Point};

    use super::GpuBufferLengths;
    use crate::{Canvas, TextContext, TextFontSystem, TextLayoutOptions, text::PreparedTextData};

    #[test]
    fn text_lengths_count_only_tiles_intersecting_glyph_bounds() {
        let mut font_system = TextFontSystem::new();
        let mut context = TextContext::new();
        let layout = context.layout(&mut font_system, TextLayoutOptions::new("MMMMMMMM", 24.0));
        if layout.is_empty() {
            return;
        }

        let mut canvas = Canvas::new(160, 64, 1.0);
        canvas.push_text_layout(&layout, Point::new(2.0, 32.0), Color::WHITE);
        let text = PreparedTextData::new(
            &canvas.text_glyphs,
            &canvas.text_runs,
            &mut font_system,
            &mut context,
        );
        let lengths = GpuBufferLengths::from_scene_with_text(&canvas, Some(&text));

        let expected = text
            .run_glyph_indices(0)
            .filter_map(|glyph_id| {
                let bounds = text.glyph_bounds(glyph_id)?;
                Some(super::bounds_tile_bbox(
                    bounds,
                    canvas.width_in_tiles(),
                    canvas.height_in_tiles(),
                ))
            })
            .map(|bbox| bbox.tile_count() as usize)
            .sum::<usize>();

        assert_eq!(lengths.coarse_glyph_capacity, expected);
    }

    #[test]
    fn text_lengths_track_whether_gpu_text_data_was_prepared() {
        let mut font_system = TextFontSystem::new();
        let mut context = TextContext::new();
        let layout = context.layout(&mut font_system, TextLayoutOptions::new("text", 24.0));
        if layout.is_empty() {
            return;
        }

        let mut canvas = Canvas::new(96, 48, 1.0);
        canvas.push_text_layout(&layout, Point::new(2.0, 28.0), Color::WHITE);
        let no_text_lengths = GpuBufferLengths::from_scene(&canvas);
        let text = PreparedTextData::new(
            &canvas.text_glyphs,
            &canvas.text_runs,
            &mut font_system,
            &mut context,
        );
        let text_lengths = GpuBufferLengths::from_scene_with_text(&canvas, Some(&text));

        assert!(!no_text_lengths.text_enabled);
        assert!(text_lengths.text_enabled);
    }
}

#[cfg(test)]
mod tests {
    use peniko::{
        Color,
        kurbo::{Affine, Rect, Shape},
    };

    use super::{
        COARSE_CHUNK_SIZE, CUMSUM_CHUNK_SIZE, GpuBufferLengths, GpuLengthOverrides, GpuScanChunk,
        GpuScanChunkRange, PersistentPathPlans, SCAN_CHUNK_SIZE, TILE_DRAW_PAGE_WORDS,
        TileDrawBins, build_cumsum_plan, build_scan_chunks, build_tile_draw_bins,
        build_tile_draw_bins_into,
    };
    use crate::{Bounds, Canvas, FillRule};

    #[test]
    fn scan_plan_records_are_gpu_word_layouts() {
        assert_eq!(std::mem::size_of::<GpuScanChunk>(), 16);
        assert_eq!(std::mem::align_of::<GpuScanChunk>(), 4);
        assert_eq!(std::mem::size_of::<GpuScanChunkRange>(), 8);
        assert_eq!(std::mem::align_of::<GpuScanChunkRange>(), 4);
    }

    #[test]
    fn scan_chunks_cover_each_backdrop_record_in_fixed_size_tiles() {
        let mut canvas = Canvas::new(
            (SCAN_CHUNK_SIZE + 17) * crate::TILE_SIZE,
            crate::TILE_SIZE,
            1.0,
        );
        canvas.push_path(
            Rect::new(
                0.0,
                0.0,
                f64::from((SCAN_CHUNK_SIZE + 17) * crate::TILE_SIZE),
                f64::from(crate::TILE_SIZE),
            )
            .to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let lengths = GpuBufferLengths::from_scene(&canvas);
        let (chunks, ranges) = build_scan_chunks(&canvas);

        assert_eq!(lengths.scan_chunk_count, 2);
        assert_eq!(chunks.len(), 2);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 2);
        assert_eq!(chunks[0].backdrop_offset, 0);
        assert_eq!(chunks[0].len, SCAN_CHUNK_SIZE);
        assert_eq!(chunks[1].backdrop_offset, SCAN_CHUNK_SIZE);
        assert_eq!(chunks[1].len, 17);
    }

    #[test]
    fn persistent_path_plans_rebuild_only_changed_path_allocations() {
        let mut canvas = Canvas::new(crate::TILE_SIZE * 4, crate::TILE_SIZE, 1.0);
        for x in [0.0, 32.0] {
            canvas.push_path(
                Rect::new(x, 0.0, x + 32.0, 16.0).to_path(0.0),
                Color::BLACK,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.0,
            );
        }
        let mut plans = PersistentPathPlans::default();
        plans.update(&canvas, None);
        let _ = plans.take_dirty();
        let first = plans.scan_ranges()[0];

        canvas.path_records[1].data_offset += 7;
        plans.update(&canvas, Some(std::slice::from_ref(&(1..2))));
        let dirty = plans.take_dirty();

        assert_eq!(plans.scan_ranges()[0], first);
        assert!(
            dirty
                .scan_chunks
                .iter()
                .all(|range| range.start >= first.end as usize)
        );
        assert_eq!(dirty.scan_ranges, Vec::<std::ops::Range<usize>>::new());
    }

    #[test]
    fn vacant_stable_path_slot_does_not_clobber_path_zero_scan_range() {
        let mut canvas = Canvas::new(crate::TILE_SIZE * 4, crate::TILE_SIZE, 1.0);
        for x in [0.0, 32.0] {
            canvas.push_path(
                Rect::new(x, 0.0, x + 32.0, 16.0).to_path(0.0),
                Color::BLACK,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.0,
            );
        }
        let mut plans = PersistentPathPlans::default();
        plans.update(&canvas, None);
        let first = plans.scan_ranges()[0];

        canvas.path_records[1] = Default::default();
        plans.update(&canvas, Some(std::slice::from_ref(&(1..2))));

        assert_eq!(plans.scan_ranges()[0], first);
        assert_eq!(plans.scan_ranges()[1], GpuScanChunkRange::default());
    }

    #[test]
    fn cumsum_plan_splits_each_backdrop_row_into_fixed_size_chunks() {
        let row_tiles = CUMSUM_CHUNK_SIZE + 17;
        let mut canvas = Canvas::new(row_tiles * crate::TILE_SIZE, crate::TILE_SIZE * 2, 1.0);
        canvas.push_path(
            Rect::new(
                0.0,
                0.0,
                f64::from(row_tiles * crate::TILE_SIZE),
                f64::from(crate::TILE_SIZE * 2),
            )
            .to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let lengths = GpuBufferLengths::from_scene(&canvas);
        let plan = build_cumsum_plan(&canvas);

        assert_eq!(lengths.cumsum_row_count, 2);
        assert_eq!(lengths.cumsum_chunk_count, 4);
        assert_eq!(plan.row_chunk_starts, vec![0, 2]);
        assert_eq!(plan.row_chunk_ends, vec![2, 4]);
        assert_eq!(
            plan.chunk_backdrop_offsets,
            vec![
                0,
                CUMSUM_CHUNK_SIZE,
                row_tiles,
                row_tiles + CUMSUM_CHUNK_SIZE
            ]
        );
        assert_eq!(
            plan.chunk_lens,
            vec![CUMSUM_CHUNK_SIZE, 17, CUMSUM_CHUNK_SIZE, 17]
        );
    }

    #[test]
    fn tile_draw_bins_keep_each_tiles_draws_in_scene_order() {
        let mut canvas = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        canvas.push_rect(
            Rect::new(16.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        canvas.push_rect(
            Rect::new(40.0, 0.0, 48.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );

        let bins = build_tile_draw_bins(&canvas);

        assert_eq!(bins.tile_draws(0), vec![0]);
        assert_eq!(bins.tile_draws(1), vec![0, 1]);
    }

    #[test]
    fn tile_draw_bins_query_region_without_scanning_unrelated_tiles() {
        let mut canvas = Canvas::new(crate::TILE_SIZE * 4, crate::TILE_SIZE * 2, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 48.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        canvas.push_rect(
            Rect::new(80.0, 0.0, 96.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let bins = build_tile_draw_bins(&canvas);

        assert_eq!(
            bins.draws_in_bounds(Bounds::new(32, 0, 64, 32), &plan.draw_order),
            vec![0]
        );
        assert!(
            bins.draws_in_bounds(Bounds::new(0, 32, 32, 64), &plan.draw_order)
                .is_empty()
        );
    }

    #[test]
    fn active_batch_ids_deduplicate_with_reusable_generation_marks() {
        let mut canvas = Canvas::new(crate::TILE_SIZE * 3, crate::TILE_SIZE, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        canvas.push_rect(
            Rect::new(16.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        canvas.push_rect(
            Rect::new(32.0, 0.0, 48.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        let mut bins = build_tile_draw_bins(&canvas);
        let batches = [7, 3, u32::MAX];

        assert_eq!(bins.active_batch_ids(&[0, 1], &batches), [3, 7]);
        assert!(bins.active_batch_ids(&[2], &batches).is_empty());
        assert_eq!(bins.active_batch_ids(&[1], &batches), [3, 7]);
    }

    #[test]
    fn fused_lengths_reuse_the_same_tile_draw_bins() {
        let mut canvas = Canvas::new_persistent(
            crate::TILE_SIZE * 2,
            crate::TILE_SIZE,
            1.0,
            crate::RetainedNodeId::for_owner(70_000),
        );
        for _ in 0..=COARSE_CHUNK_SIZE {
            canvas.push_rect(
                Rect::new(0.0, 0.0, 16.0, 16.0),
                crate::Radius::ZERO,
                Color::BLACK,
            );
        }
        canvas.push_rect(
            Rect::new(16.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );

        let mut bins = TileDrawBins::default();
        let mut cursors = Vec::new();
        let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let lengths = GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
            &canvas,
            None,
            &plan,
            &mut bins,
            &mut cursors,
            false,
            GpuLengthOverrides::default(),
        );

        assert_eq!(lengths.tile_draw_index_count, bins.upload_index_count());
        assert!(bins.upload_indices().len() < bins.draw_indices.len());
        assert_eq!(
            lengths.tile_draw_chunk_count,
            bins.records
                .iter()
                .map(|record| record.end as usize)
                .map(|count| count.div_ceil(COARSE_CHUNK_SIZE as usize))
                .sum::<usize>()
        );
        assert_eq!(bins.records[0].end, COARSE_CHUNK_SIZE + 1);
        assert_eq!(bins.records[1].end, 1);
    }

    #[test]
    fn tile_bins_switch_from_transient_flat_upload_back_to_persistent_pages() {
        let mut transient = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
        transient.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        transient.push_rect(
            Rect::new(16.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        let mut bins = TileDrawBins::default();
        let mut cursors = Vec::new();
        let transient_plan = transient.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
            &transient,
            None,
            &transient_plan,
            &mut bins,
            &mut cursors,
            false,
            GpuLengthOverrides::default(),
        );

        let mut persistent = Canvas::new_persistent(
            crate::TILE_SIZE * 2,
            crate::TILE_SIZE,
            1.0,
            crate::RetainedNodeId::for_owner(70_001),
        );
        persistent.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        persistent.push_rect(
            Rect::new(16.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        let persistent_plan = persistent.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let lengths = GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
            &persistent,
            None,
            &persistent_plan,
            &mut bins,
            &mut cursors,
            false,
            GpuLengthOverrides::default(),
        );

        assert_eq!(bins.tile_draws(0), [0]);
        assert_eq!(bins.tile_draws(1), [1]);
        assert_eq!(lengths.tile_draw_chunk_count, 2);
        assert_eq!(lengths.tile_draw_index_count, 2);
    }

    #[test]
    fn tile_page_capacity_includes_fused_layer_stack_particles() {
        let mut canvas = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
        canvas.push_opacity_layer(
            Rect::new(0.0, 0.0, 32.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
            0.5,
        );
        canvas.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        canvas.push_rect(
            Rect::new(16.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        canvas.pop_layer();
        let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let mut bins = TileDrawBins::default();
        let mut cursors = Vec::new();
        let lengths = GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
            &canvas,
            None,
            &plan,
            &mut bins,
            &mut cursors,
            false,
            GpuLengthOverrides::default(),
        );

        // Two tiles each require begin-opacity, draw, end-opacity, and terminator particles.
        assert!(lengths.coarse_ptcl_capacity >= 8);
    }

    #[test]
    fn tile_draw_chunk_count_sums_per_tile_draw_chunks() {
        let mut canvas = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
        for _ in 0..=COARSE_CHUNK_SIZE {
            canvas.push_rect(
                Rect::new(0.0, 0.0, 16.0, 16.0),
                crate::Radius::ZERO,
                Color::BLACK,
            );
        }
        canvas.push_rect(
            Rect::new(16.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );

        let lengths = GpuBufferLengths::from_scene(&canvas);

        assert_eq!(lengths.tile_draw_index_count, 3 * TILE_DRAW_PAGE_WORDS);
        assert_eq!(lengths.tile_draw_chunk_count, 3);
    }

    #[test]
    fn incremental_tile_pages_rewrite_only_old_and_new_bounds_tiles() {
        let mut initial = Canvas::new(crate::TILE_SIZE * 3, crate::TILE_SIZE, 1.0);
        initial.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        initial.push_rect(
            Rect::new(32.0, 0.0, 48.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        let plan = initial.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let mut bins = TileDrawBins::default();
        let mut cursors = Vec::new();
        build_tile_draw_bins_into(&initial, &plan, &mut bins, &mut cursors);
        let _ = bins.take_dirty();
        let unaffected_page = bins.records[2].start;
        let unaffected_words = bins.draw_indices[unaffected_page as usize * TILE_DRAW_PAGE_WORDS
            ..(unaffected_page as usize + 1) * TILE_DRAW_PAGE_WORDS]
            .to_vec();

        let mut moved = Canvas::new(crate::TILE_SIZE * 3, crate::TILE_SIZE, 1.0);
        moved.push_rect(
            Rect::new(16.0, 0.0, 32.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        moved.push_rect(
            Rect::new(32.0, 0.0, 48.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        assert!(bins.update_changed(
            &moved.draw_records,
            &plan.draw_order,
            None,
            (3, 1),
            std::slice::from_ref(&(0..1)),
        ));
        let (_, dirty_records, dirty_pages) = bins.take_dirty();

        assert_eq!(bins.tile_draws(0), Vec::<u32>::new());
        assert_eq!(bins.tile_draws(1), vec![0]);
        assert_eq!(bins.tile_draws(2), vec![1]);
        assert_eq!(dirty_records, vec![0, 1]);
        assert!(!dirty_pages.contains(&unaffected_page));
        assert_eq!(
            &bins.draw_indices[unaffected_page as usize * TILE_DRAW_PAGE_WORDS
                ..(unaffected_page as usize + 1) * TILE_DRAW_PAGE_WORDS],
            unaffected_words
        );
    }

    #[test]
    fn dirty_draw_with_stable_bounds_and_rank_does_not_rewrite_tile_pages() {
        let mut canvas = Canvas::new(crate::TILE_SIZE * 2, crate::TILE_SIZE, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
        let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let mut bins = TileDrawBins::default();
        let mut cursors = Vec::new();
        build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut cursors);
        let _ = bins.take_dirty();
        let compact_indices = bins.upload_index_count();

        assert!(bins.update_changed(
            &canvas.draw_records,
            &plan.draw_order,
            None,
            (2, 1),
            std::slice::from_ref(&(0..1)),
        ));
        let (full, dirty_records, dirty_pages) = bins.take_dirty();
        assert!(!full);
        assert!(dirty_records.is_empty());
        assert!(dirty_pages.is_empty());
        assert_eq!(bins.upload_index_count(), compact_indices);
        assert!(bins.upload_indices().len() < bins.draw_indices.len());
    }

    #[test]
    fn tile_page_arena_compacts_after_fragmentation_exceeds_threshold() {
        let mut initial = Canvas::new(crate::TILE_SIZE * 20, crate::TILE_SIZE, 1.0);
        let mut moved = Canvas::new(crate::TILE_SIZE * 20, crate::TILE_SIZE, 1.0);
        for tile in 0..20 {
            initial.push_rect(
                Rect::new((tile * 16) as f64, 0.0, (tile * 16 + 16) as f64, 16.0),
                crate::Radius::ZERO,
                Color::BLACK,
            );
            let x = if tile < 8 { 1000.0 } else { (tile * 16) as f64 };
            moved.push_rect(
                Rect::new(x, 0.0, x + 16.0, 16.0),
                crate::Radius::ZERO,
                Color::BLACK,
            );
        }
        let plan = initial.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let mut bins = TileDrawBins::default();
        let mut cursors = Vec::new();
        build_tile_draw_bins_into(&initial, &plan, &mut bins, &mut cursors);
        let _ = bins.take_dirty();

        assert!(bins.update_changed(
            &moved.draw_records,
            &plan.draw_order,
            None,
            (20, 1),
            std::slice::from_ref(&(0..8)),
        ));
        let (full, _, _) = bins.take_dirty();
        assert!(full);
        assert_eq!(bins.compactions(), 1);
        assert_eq!(bins.active_page_count(), 12);
        assert_eq!(bins.draw_indices.len(), 12 * TILE_DRAW_PAGE_WORDS);
    }
}
