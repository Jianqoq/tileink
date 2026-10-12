//! Particle, glyph, layer-stack and filter scratch capacity contracts.
#[cfg(test)]
use super::GpuBufferLengths;
use crate::{
    canvas::Canvas,
    shared::{
        bounds::{Bounds, PixelBounds, TileBbox},
        draw_record::{DrawRecord, DrawTag},
        execution::{ExecOp, ExecPlan, LayerStackEntry, OffscreenLayer},
        layer::filter::{Filter, FilterInput, FilterPrimitive, FilterPrimitiveKind},
    },
    text::PreparedTextData,
};

pub(super) fn draw_ptcl_capacity(draw: &DrawRecord, bbox: TileBbox) -> usize {
    let tiles = bbox.tile_count() as usize;
    let draw_particle = usize::from(
        (draw.has_path() || draw.has_analytic_geometry() || draw.glyph_run_id().is_some())
            && matches!(
                draw.tag(),
                DrawTag::Brush | DrawTag::PathGlyph | DrawTag::Clip
            ),
    );
    let group_begin = usize::from(
        draw.has_path()
            && matches!(
                draw.tag(),
                DrawTag::Opacity | DrawTag::Blend | DrawTag::Isolate
            ),
    );
    let layer_end = usize::from(
        (matches!(draw.tag(), DrawTag::Clip) && (draw.has_path() || draw.sdf_range().is_some()))
            || (draw.has_path()
                && matches!(
                    draw.tag(),
                    DrawTag::Opacity | DrawTag::Blend | DrawTag::Isolate
                )),
    );
    tiles * (draw_particle + group_begin + layer_end)
}

pub(super) fn coarse_glyph_capacity(
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
                        transformed_glyph_bounds(draw, text.glyph_bounds(glyph_id)?),
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
            let glyph_bbox = bounds_tile_bbox(
                transformed_glyph_bounds(draw, text.glyph_bounds(glyph_id)?),
                width,
                height,
            );
            Some(tile_bbox_intersection_count(draw_bbox, glyph_bbox))
        })
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

fn transformed_glyph_bounds(draw: &DrawRecord, bounds: Bounds) -> Bounds {
    // Match Metal/HLSL coarse candidates: bilinear sampling can read one local
    // texel beyond the bitmap. Nearest-only capacity drops later glyphs/tiles.
    let bounds = bounds.outset(1);
    let bounds = draw.transform.transform_bounds(PixelBounds {
        x0: bounds.x0,
        y0: bounds.y0,
        x1: bounds.x1,
        y1: bounds.y1,
    });
    Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
}

fn tile_bbox_intersection_count(a: TileBbox, b: TileBbox) -> usize {
    let x0 = a.x0.max(b.x0);
    let y0 = a.y0.max(b.y0);
    let x1 = a.x1.min(b.x1);
    let y1 = a.y1.min(b.y1);
    x1.saturating_sub(x0) as usize * y1.saturating_sub(y0) as usize
}

pub(super) fn coarse_ptcl_capacity(
    canvas: &Canvas,
    width_in_tiles: u32,
    height_in_tiles: u32,
) -> usize {
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
            } => match layer {
                OffscreenLayer::Isolate { .. }
                | OffscreenLayer::Opacity { .. }
                | OffscreenLayer::Blend { .. } => {
                    let source_held = held + 1;
                    max_count = max_count.max(source_held + 1);
                    max_count = max_count.max(max_scratch_for_ops(children, source_held));
                }
                OffscreenLayer::Filter { filter, .. } => {
                    let source_held = held + 1;
                    max_count = max_count.max(source_held + filter_scratch_extra(filter));
                    if !outer_stack.is_empty() {
                        max_count = max_count.max(source_held);
                    }
                    max_count = max_count.max(max_scratch_for_ops(children, source_held));
                }
                OffscreenLayer::Backdrop { filter, .. } => {
                    // Retained backdrops keep both filtered output and the
                    // painter-order source history live while rerendering.
                    let backdrop_held = held + 1 + usize::from(retained_id.is_some());
                    max_count = max_count.max(backdrop_held + filter_scratch_extra(filter));
                    max_count = max_count.max(backdrop_held + 1);
                    let content_held = held + 1;
                    max_count = max_count.max(max_scratch_for_ops(children, content_held));
                }
            },
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                let content_held = held + 1;
                max_count = max_count.max(max_scratch_for_ops(content, content_held));
                let mask_source_held = held + 2;
                max_count = max_count.max(mask_source_held + 1);
                max_count = max_count.max(max_scratch_for_ops(mask, mask_source_held));
            }
            ExecOp::DrawBatch { .. } => {}
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
        // Shader pipeline partial outputs may need a cropped copy.
        Filter::ShaderPipeline(_)
        | Filter::ConvolveMatrix(_)
        | Filter::DiffuseLighting(_)
        | Filter::SpecularLighting(_)
        | Filter::Offset { .. } => 1,
        Filter::Morphology { .. } => 2,
        Filter::DropShadow { std_dev, .. } => 1 + usize::from(std_dev.max(0.0) > 0.0),
        _ => 0,
    }
}

fn graph_scratch_extra(primitives: &[FilterPrimitive]) -> usize {
    let source_alpha = primitives.iter().any(|primitive| {
        // Vector-input primitives use their own edges, not the shared fields.
        match &primitive.kind {
            FilterPrimitiveKind::ShaderPipeline { inputs, .. }
            | FilterPrimitiveKind::Merge { inputs } => inputs.contains(&FilterInput::SourceAlpha),
            _ => {
                primitive.input == FilterInput::SourceAlpha
                    || primitive.input2 == Some(FilterInput::SourceAlpha)
            }
        }
    });
    let unary_temp = primitives
        .iter()
        .filter_map(|primitive| match &primitive.kind {
            FilterPrimitiveKind::Filter(filter) => Some(1 + filter_scratch_extra(filter)),
            FilterPrimitiveKind::ShaderPipeline { .. } => Some(1),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    primitives.len() + usize::from(source_alpha) + unary_temp
}

#[cfg(test)]
#[path = "text_length_tests.rs"]
mod tests;
