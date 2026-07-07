#![cfg_attr(not(feature = "wgpu"), allow(dead_code))]

use bytemuck::{Pod, Zeroable};

use crate::{
    canvas::Canvas,
    shared::{
        bounds::{Bounds, PixelBounds, TileBbox},
        draw_record::{DrawRecord, DrawTag},
        execution::{ExecOp, ExecPlan, LayerStackEntry},
        gpu_coarse::TileDrawRecord,
        layer::{
            Layer,
            filter::{Filter, FilterInput, FilterPrimitive, FilterPrimitiveKind},
        },
    },
    text::PreparedTextData,
};

pub(crate) const SCAN_CHUNK_SIZE: u32 = 256;
pub(crate) const CUMSUM_CHUNK_SIZE: u32 = 256;
pub(crate) const COARSE_CHUNK_SIZE: u32 = 256;
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
    pub text_run_count: usize,
    pub text_glyph_count: usize,
    pub tiles_width: usize,
    pub tiles_height: usize,
    pub tile_count: usize,
    pub image_pixels: usize,
}

impl GpuBufferLengths {
    pub(crate) fn from_scene(canvas: &Canvas) -> Self {
        Self::from_scene_with_text(canvas, None)
    }

    pub(crate) fn from_scene_with_text(canvas: &Canvas, text: Option<&PreparedTextData>) -> Self {
        let tiles_width = canvas.width_in_tiles() as usize;
        let tiles_height = canvas.height_in_tiles() as usize;
        let tile_count = tiles_width * tiles_height;
        let coarse_ptcl_capacity =
            coarse_ptcl_capacity(canvas, tiles_width as u32, tiles_height as u32);
        let coarse_glyph_capacity =
            coarse_glyph_capacity(canvas, text, tiles_width as u32, tiles_height as u32);
        let tile_draw_index_count =
            tile_draw_index_count(canvas, tiles_width as u32, tiles_height as u32);
        let tile_draw_chunk_count =
            tile_draw_chunk_count(canvas, tiles_width as u32, tiles_height as u32);
        Self {
            line_count: canvas.lines.len(),
            path_count: canvas.path_records.len(),
            draw_count: canvas.draw_records.len(),
            backdrop_record_count: canvas.path_records.len(),
            backdrop_len: canvas.backdrop_pool_capacity as usize,
            segment_capacity: canvas.tile_cnt as usize,
            scan_chunk_count: canvas
                .path_records
                .iter()
                .map(|record| record.data_len.div_ceil(SCAN_CHUNK_SIZE) as usize)
                .sum(),
            cumsum_chunk_count: canvas
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
            cumsum_row_count: canvas
                .path_records
                .iter()
                .map(|record| {
                    let stride = record.tile_x1.saturating_sub(record.tile_x0);
                    let height = record.tile_y1.saturating_sub(record.tile_y0);
                    if stride == 0 { 0 } else { height as usize }
                })
                .sum(),
            coarse_chunk_count: tile_count.div_ceil(COARSE_CHUNK_SIZE as usize),
            coarse_ptcl_capacity,
            coarse_glyph_capacity,
            tile_draw_index_count,
            tile_draw_chunk_count,
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TileDrawBins {
    pub(crate) records: Vec<TileDrawRecord>,
    pub(crate) draw_indices: Vec<u32>,
}

#[cfg(test)]
pub(crate) fn build_tile_draw_bins(canvas: &Canvas) -> TileDrawBins {
    let mut bins = TileDrawBins::default();
    let mut cursors = Vec::new();
    build_tile_draw_bins_into(canvas, &mut bins, &mut cursors);
    bins
}

pub(crate) fn build_tile_draw_bins_into(
    canvas: &Canvas,
    bins: &mut TileDrawBins,
    cursors: &mut Vec<u32>,
) {
    build_tile_draw_bins_for_draws_into(
        &canvas.draw_records,
        (canvas.width_in_tiles(), canvas.height_in_tiles()),
        bins,
        cursors,
    );
}

pub(crate) fn build_tile_draw_bins_for_draws_into(
    draw_records: &[DrawRecord],
    tiles_size: (u32, u32),
    bins: &mut TileDrawBins,
    cursors: &mut Vec<u32>,
) {
    let (width_in_tiles, height_in_tiles) = tiles_size;
    let tile_count = width_in_tiles as usize * height_in_tiles as usize;

    bins.records.clear();
    bins.records.resize(tile_count, TileDrawRecord::default());
    bins.draw_indices.clear();
    cursors.clear();
    cursors.resize(tile_count, 0);

    for draw in draw_records {
        for_tile_in_bbox(
            draw.tile_bbox(width_in_tiles, height_in_tiles),
            width_in_tiles,
            |tile_ix| {
                bins.records[tile_ix].end += 1;
            },
        );
    }

    let mut cursor = 0;
    for record in &mut bins.records {
        record.start = cursor;
        cursor += record.end;
        record.end = cursor;
    }

    bins.draw_indices.resize(cursor as usize, 0);
    for (cursor, record) in cursors.iter_mut().zip(&bins.records) {
        *cursor = record.start;
    }

    for (draw_ix, draw) in draw_records.iter().enumerate() {
        for_tile_in_bbox(
            draw.tile_bbox(width_in_tiles, height_in_tiles),
            width_in_tiles,
            |tile_ix| {
                let dst = cursors[tile_ix] as usize;
                bins.draw_indices[dst] = draw_ix as u32;
                cursors[tile_ix] += 1;
            },
        );
    }

    debug_assert!(
        cursors
            .iter()
            .zip(&bins.records)
            .all(|(cursor, record)| *cursor == record.end)
    );
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

fn tile_draw_index_count(canvas: &Canvas, width_in_tiles: u32, height_in_tiles: u32) -> usize {
    canvas
        .draw_records
        .iter()
        .map(|draw| draw.tile_bbox(width_in_tiles, height_in_tiles).tile_count() as usize)
        .sum()
}

fn tile_draw_chunk_count(canvas: &Canvas, width_in_tiles: u32, height_in_tiles: u32) -> usize {
    let tile_count = width_in_tiles as usize * height_in_tiles as usize;
    let mut counts = vec![0usize; tile_count];
    for draw in &canvas.draw_records {
        for_tile_in_bbox(
            draw.tile_bbox(width_in_tiles, height_in_tiles),
            width_in_tiles,
            |tile_ix| counts[tile_ix] += 1,
        );
    }
    counts
        .into_iter()
        .map(|count| count.div_ceil(COARSE_CHUNK_SIZE as usize))
        .sum()
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
                    let backdrop_held = held + 1;
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
pub(crate) fn build_scan_chunks(canvas: &Canvas) -> (Vec<GpuScanChunk>, Vec<GpuScanChunkRange>) {
    let mut chunks = Vec::with_capacity(GpuBufferLengths::from_scene(canvas).scan_chunk_count);
    let mut ranges = Vec::with_capacity(canvas.path_records.len());
    build_scan_chunks_into(canvas, &mut chunks, &mut ranges);
    (chunks, ranges)
}

pub(crate) fn build_scan_chunks_into(
    canvas: &Canvas,
    chunks: &mut Vec<GpuScanChunk>,
    ranges: &mut Vec<GpuScanChunkRange>,
) {
    chunks.clear();
    chunks.reserve(GpuBufferLengths::from_scene(canvas).scan_chunk_count);
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

#[cfg(test)]
pub(crate) fn build_cumsum_plan(canvas: &Canvas) -> GpuCumsumPlan {
    let mut plan = GpuCumsumPlan::default();
    build_cumsum_plan_into(canvas, &mut plan);
    plan
}

pub(crate) fn build_cumsum_plan_into(canvas: &Canvas, plan: &mut GpuCumsumPlan) {
    let lengths = GpuBufferLengths::from_scene(canvas);
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
}

#[cfg(test)]
mod tests {
    use peniko::{
        Color,
        kurbo::{Affine, Rect, Shape},
    };

    use super::{
        COARSE_CHUNK_SIZE, CUMSUM_CHUNK_SIZE, GpuBufferLengths, SCAN_CHUNK_SIZE, build_cumsum_plan,
        build_scan_chunks, build_tile_draw_bins,
    };
    use crate::{Canvas, FillRule};

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

        assert_eq!(
            bins.records
                .iter()
                .map(|record| record.start)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            bins.records
                .iter()
                .map(|record| record.end)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(bins.draw_indices, vec![0, 0, 1]);
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

        assert_eq!(
            lengths.tile_draw_index_count,
            COARSE_CHUNK_SIZE as usize + 2
        );
        assert_eq!(lengths.tile_draw_chunk_count, 3);
    }
}
