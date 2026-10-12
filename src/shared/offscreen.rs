use std::rc::Rc;
mod canvas;
mod selection;
mod space;
use canvas::translated_scene_for_draws;
use selection::local_draw_selection;
use space::LocalSpace;

use std::collections::{HashMap, HashSet};

use peniko::kurbo::{Affine, Rect};

use crate::{
    canvas::Canvas,
    shared::{
        bounds::{Bounds, PixelBounds, TileBbox},
        brush::{Brush, decode_encoded_brush, push_encoded_brush},
        draw_record::DrawRecord,
        execution::{ExecOp, ExecPlan, OffscreenLayer},
        gpu_sdf::{decode_sdf, decode_sdf_shadow, push_encoded_sdf, push_encoded_sdf_shadow},
        layer::{
            filter::{Filter, FilterPrimitive, FilterPrimitiveKind, Turbulence},
            mask::Mask,
            region::Region,
        },
        line::Line,
        path::PathRecord,
        sdf::{Sdf, SdfShadow},
    },
};

pub(crate) struct LocalOffscreenCanvas {
    pub(crate) canvas: Canvas,
    pub(crate) plan: ExecPlan,
    pub(crate) children: Vec<ExecOp>,
}

pub(crate) fn local_offscreen_scene(
    canvas: &Canvas,
    plan: &ExecPlan,
    children: &[ExecOp],
    bounds: Bounds,
    candidate_draws: &[u32],
) -> LocalOffscreenCanvas {
    let local = LocalSpace::new(bounds);
    let selection = local_draw_selection(canvas, plan, children, candidate_draws, bounds);
    let (canvas, draw_map) = translated_scene_for_draws(canvas, local, &selection.draws);
    let local_children =
        translate_exec_ops_to_local(children, local, &selection.batch_draws, &draw_map);
    let mut plan = ExecPlan {
        ops: local_children,
        layer_stack_data: plan.layer_stack_data.clone(),
        draw_order: Rc::default(),
        draw_batch_ids: Rc::default(),
        retained_batch_ids: HashMap::new(),
        layer_stack_locations: HashMap::new(),
        direct_root_batch_ops: None,
    };
    plan.coalesce_draw_batches();
    for entry in &mut plan.layer_stack_data {
        *entry = remap_layer_stack_entry(*entry, &draw_map);
    }
    plan.finalize_draw_batches(canvas.draw_records.len());
    let children = plan.ops.clone();
    LocalOffscreenCanvas {
        canvas,
        plan,
        children,
    }
}

pub(crate) fn local_filter(filter: &Filter, bounds: Bounds) -> Filter {
    translate_filter_to_local(filter, LocalSpace::new(bounds))
}

fn translate_exec_ops_to_local(
    ops: &[ExecOp],
    local: LocalSpace,
    batch_draws: &HashMap<u32, Vec<usize>>,
    draw_map: &HashMap<usize, usize>,
) -> Vec<ExecOp> {
    ops.iter()
        .map(|op| match op {
            ExecOp::DrawBatch {
                batch_id,
                layer_stack,
                owners,
                ..
            } => ExecOp::DrawBatch {
                draws: std::rc::Rc::new(
                    batch_draws[batch_id]
                        .iter()
                        .map(|draw| draw_map[draw])
                        .collect(),
                ),
                batch_id: *batch_id,
                layer_stack: layer_stack.clone(),
                owners: std::rc::Rc::clone(owners),
            },
            ExecOp::OffscreenLayer {
                retained_id,
                layer,
                outer_stack,
                children,
            } => ExecOp::OffscreenLayer {
                retained_id: *retained_id,
                layer: translate_layer_to_local(layer, local, draw_map),
                outer_stack: outer_stack.clone(),
                children: translate_exec_ops_to_local(children, local, batch_draws, draw_map),
            },
            ExecOp::OffscreenMaskLayer {
                retained_id,
                layer,
                outer_stack,
                content,
                mask,
            } => ExecOp::OffscreenMaskLayer {
                retained_id: *retained_id,
                layer: translate_mask_to_local(layer, local),
                outer_stack: outer_stack.clone(),
                content: translate_exec_ops_to_local(content, local, batch_draws, draw_map),
                mask: translate_exec_ops_to_local(mask, local, batch_draws, draw_map),
            },
        })
        .collect()
}

fn remap_layer_stack_entry(
    entry: crate::shared::execution::LayerStackEntry,
    draw_map: &HashMap<usize, usize>,
) -> crate::shared::execution::LayerStackEntry {
    use crate::shared::execution::LayerStackEntry;
    let remap = |draw: u32| draw_map[&(draw as usize)] as u32;
    match entry {
        LayerStackEntry::Clip { draw } => LayerStackEntry::Clip { draw: remap(draw) },
        LayerStackEntry::Opacity { draw, opacity } => LayerStackEntry::Opacity {
            draw: remap(draw),
            opacity,
        },
        LayerStackEntry::Blend { draw, mode } => LayerStackEntry::Blend {
            draw: remap(draw),
            mode,
        },
    }
}

fn translate_layer_to_local(
    layer: &OffscreenLayer,
    local: LocalSpace,
    draw_map: &HashMap<usize, usize>,
) -> OffscreenLayer {
    match layer {
        OffscreenLayer::Isolate { draw } => OffscreenLayer::Isolate {
            draw: draw_map[draw],
        },
        OffscreenLayer::Opacity { draw, opacity } => OffscreenLayer::Opacity {
            draw: draw_map[draw],
            opacity: *opacity,
        },
        OffscreenLayer::Blend { draw, mode } => OffscreenLayer::Blend {
            draw: draw_map[draw],
            mode: *mode,
        },
        OffscreenLayer::Filter {
            filter,
            sample_region,
        } => OffscreenLayer::Filter {
            filter: translate_filter_to_local(filter, local),
            sample_region: translate_region_to_local(sample_region, local),
        },
        OffscreenLayer::Backdrop {
            filter,
            sample_region,
        } => OffscreenLayer::Backdrop {
            filter: translate_filter_to_local(filter, local),
            sample_region: translate_region_to_local(sample_region, local),
        },
    }
}

fn translate_mask_to_local(mask: &Mask, local: LocalSpace) -> Mask {
    Mask {
        region: translate_region_to_local(&mask.region, local),
        kind: mask.kind,
    }
}

fn translate_filter_to_local(filter: &Filter, local: LocalSpace) -> Filter {
    match filter {
        Filter::ProgressiveBlur(blur) => Filter::ProgressiveBlur(blur.translated(
            peniko::kurbo::Vec2::new(-f64::from(local.surface.x0), -f64::from(local.surface.y0)),
        )),
        Filter::Chain {
            filters,
            fixed_region,
        } => Filter::Chain {
            filters: filters
                .iter()
                .map(|filter| translate_filter_to_local(filter, local))
                .collect(),
            fixed_region: *fixed_region,
        },
        Filter::Graph {
            primitives,
            fixed_region,
        } => Filter::Graph {
            primitives: primitives
                .iter()
                .map(|primitive| FilterPrimitive {
                    input: primitive.input,
                    input2: primitive.input2,
                    region: local.bounds(primitive.region),
                    linear_rgb: primitive.linear_rgb,
                    kind: translate_primitive_kind_to_local(&primitive.kind, local),
                })
                .collect(),
            fixed_region: *fixed_region,
        },
        Filter::ShaderPipeline(pipeline) => {
            let mut pipeline = pipeline.clone();
            pipeline.transform = local.brush_transform(pipeline.transform);
            Filter::ShaderPipeline(pipeline)
        }
        Filter::Flood { brush } => Filter::Flood {
            brush: local.brush(brush.clone()),
        },
        Filter::DropShadow {
            offset_x,
            offset_y,
            std_dev,
            brush,
        } => Filter::DropShadow {
            offset_x: *offset_x,
            offset_y: *offset_y,
            std_dev: *std_dev,
            brush: local.brush(brush.clone()),
        },
        _ => filter.clone(),
    }
}

fn translate_primitive_kind_to_local(
    kind: &FilterPrimitiveKind,
    local: LocalSpace,
) -> FilterPrimitiveKind {
    match kind {
        FilterPrimitiveKind::ShaderPipeline { pipeline, inputs } => {
            let mut pipeline = pipeline.clone();
            pipeline.transform = local.brush_transform(pipeline.transform);
            FilterPrimitiveKind::ShaderPipeline {
                pipeline,
                inputs: inputs.clone(),
            }
        }
        FilterPrimitiveKind::Filter(filter) => {
            FilterPrimitiveKind::Filter(Box::new(translate_filter_to_local(filter, local)))
        }
        FilterPrimitiveKind::Image { brush } => FilterPrimitiveKind::Image {
            brush: local.brush(brush.clone()),
        },
        FilterPrimitiveKind::Tile { source_region } => FilterPrimitiveKind::Tile {
            source_region: local.bounds(*source_region),
        },
        FilterPrimitiveKind::Turbulence(turbulence) => local.turbulence(*turbulence),
        _ => kind.clone(),
    }
}

fn translate_region_to_local(region: &Region, local: LocalSpace) -> Region {
    match region {
        Region::Rect { rect, radius } => Region::rect(local.rect(*rect), *radius),
        Region::Path {
            path,
            transform,
            tolerance,
        } => Region::path(path.clone(), local.transform(*transform), *tolerance),
    }
}

#[cfg(test)]
mod tests;
