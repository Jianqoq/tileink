//! Painter-ordered tile membership and persistent upload storage.
//! No statistics are maintained for retired kernel-selection policies.
use super::{
    COARSE_CHUNK_SIZE, TILE_DRAW_PAGE_WORDS,
    capacity::{draw_ptcl_capacity, plan_stack_depths},
};
use crate::{
    canvas::{Canvas, PainterKey},
    shared::{
        bounds::{Bounds, PixelBounds, TileBbox},
        dense_set::{DenseIndexSet, GenerationMarks},
        draw_record::DrawRecord,
        execution::ExecPlan,
        gpu_coarse::TileDrawRecord,
    },
};
use std::collections::HashSet;

const TILE_DRAW_FLAT_FLAG: u32 = 1 << 31;
const RETAINED_TILE_DIRTY_CAPACITY: usize = 1_024;

mod active_batches;
pub(crate) use active_batches::ActiveBatches;

mod build;
mod pages;
mod update;

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
    visited_draws: GenerationMarks,
    membership_changed: DenseIndexSet,
    affected_tiles: DenseIndexSet,
}

impl TileDrawBins {
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

    /// Returns conservative painter-ordered candidates for a pixel region. Queries within the
    /// indexed domain avoid scanning the scene draw table. Persistent and transient bins share
    /// the same page/flat query path; exterior queries preserve all possible filter sources.
    pub(crate) fn draws_in_bounds(
        &self,
        bounds: Bounds,
        draw_order: &[u32],
        painter_keys: Option<&[PainterKey]>,
    ) -> Vec<u32> {
        // Filters can move off-canvas source pixels into visible output. Tile
        // membership covers only this indexed domain, so an exterior query must
        // leave exact source clipping to the localizer instead of dropping draws.
        let indexed = Bounds::canvas(
            self.tiles_size.0 * crate::TILE_SIZE,
            self.tiles_size.1 * crate::TILE_SIZE,
        );
        if bounds.intersect(indexed) != bounds {
            // Retained fragment/flat edits deliberately reuse execution structure. Its original
            // draw_order can omit new draws; the current painter owns the complete live order.
            if let Some(keys) = painter_keys {
                let mut draws = keys
                    .iter()
                    .enumerate()
                    .filter_map(|(draw, key)| (key.path[0] != u128::MAX).then_some(draw as u32))
                    .collect::<Vec<_>>();
                draws.sort_unstable_by(|a, b| keys[*a as usize].cmp(&keys[*b as usize]));
                return draws;
            }
            return draw_order.to_vec();
        }
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
        // Persistent bins own complete ranks; transient retained bins use their current owner.
        // Immediate transient bins carry no ranks and use the supplied compiled draw sequence.
        let ranks =
            painter_keys.or_else(|| self.page_arena_valid.then_some(self.draw_ranks.as_slice()));
        if let Some(ranks) = ranks {
            let mut draws = candidates.into_iter().collect::<Vec<_>>();
            draws.sort_unstable_by(|a, b| ranks[*a as usize].cmp(&ranks[*b as usize]));
            draws
        } else {
            draw_order
                .iter()
                .copied()
                .filter(|draw| candidates.contains(draw))
                .collect()
        }
    }

    pub(super) fn coarse_ptcl_capacity(
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TileDrawCounts {
    pub(crate) index_count: usize,
    pub(crate) chunk_count: usize,
}

#[cfg(test)]
fn build_tile_draw_bins(canvas: &Canvas) -> TileDrawBins {
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    build_tile_draw_bins_into(canvas, &plan, &mut bins, &mut cursors);
    bins
}

pub(super) fn build_tile_draw_bins_into(
    canvas: &Canvas,
    plan: &ExecPlan,
    bins: &mut TileDrawBins,
    cursors: &mut Vec<u32>,
) -> TileDrawCounts {
    build_tile_draw_bins_for_draws_into(
        &canvas.draw_records,
        &plan.draw_order,
        canvas
            .retained_painter()
            .map(|painter| painter.keys.as_slice()),
        (canvas.width_in_tiles(), canvas.height_in_tiles()),
        bins,
        cursors,
    )
}

fn build_tile_draw_bins_for_draws_into(
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

pub(super) fn for_tile_in_bbox(
    mut bbox: TileBbox,
    width_in_tiles: u32,
    mut visit: impl FnMut(usize),
) {
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

#[cfg(test)]
mod tests;
