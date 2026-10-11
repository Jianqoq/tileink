//! Canvas-derived native buffer extents and current tile-index update policy.
use super::{
    COARSE_CHUNK_SIZE, CUMSUM_CHUNK_SIZE, SCAN_CHUNK_SIZE,
    capacity::{coarse_glyph_capacity, coarse_ptcl_capacity},
    tile_bins::{TileDrawBins, TileDrawCounts, build_tile_draw_bins_into},
};
#[cfg(test)]
use super::{TILE_DRAW_PAGE_WORDS, tile_bins::for_tile_in_bbox};
#[cfg(test)]
use crate::shared::draw_record::DrawRecord;
use crate::{canvas::Canvas, shared::execution::ExecPlan, text::PreparedTextData};

/// Canvas-derived fixed capacities for GPU buffers.
///
/// GPU compute stages cannot grow vectors while dispatching. This plan keeps
/// allocation sizes explicit for native upload paths
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
        let surface_changed = canvas
            .buffer_changes
            .as_ref()
            .is_some_and(|changes| changes.surface_changed);
        let dense_spatial_change = !surface_changed
            && canvas.persistent_root.is_some()
            && canvas.buffer_changes.as_ref().is_some_and(|changes| {
                bins.spatial_change_is_dense(
                    &canvas.draw_records,
                    (tiles_width as u32, tiles_height as u32),
                    &changes.draws,
                )
            });
        let updated = !surface_changed
            && !dense_spatial_change
            && (incremental || canvas.painter_keys.is_some())
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
        let tile_draw_counts =
            if (surface_changed && canvas.persistent_root.is_some()) || dense_spatial_change {
                // A resized target is fully redrawn and another resize invalidates the viewport index
                // immediately. Build the compact GPU bins directly; `reset_transient` deliberately
                // invalidates the reverse index so the first later incremental mutation rebuilds an
                // exact persistent baseline before applying dirty ranges.
                bins.reset_transient(
                    &canvas.draw_records,
                    &plan.draw_order,
                    canvas.painter_keys.as_deref(),
                    tiles_size,
                    cursors,
                    canvas.persistent_root.is_some(),
                );
                TileDrawCounts {
                    index_count: bins.upload_index_count(),
                    chunk_count: bins.active_page_count(),
                }
            } else if updated {
                TileDrawCounts {
                    index_count: bins.upload_index_count(),
                    chunk_count: bins.active_page_count(),
                }
            } else if canvas.persistent_root.is_none()
                && canvas.buffer_changes.is_none()
                && canvas.painter_keys.is_none()
            {
                bins.reset_transient(
                    &canvas.draw_records,
                    &plan.draw_order,
                    None,
                    tiles_size,
                    cursors,
                    false,
                );
                TileDrawCounts {
                    index_count: bins.upload_index_count(),
                    chunk_count: bins.active_page_count(),
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
