mod canvas;
mod selection;
use canvas::translated_scene_for_draws;
use selection::local_draw_selection;

use std::collections::{HashMap, HashSet};

use peniko::kurbo::{Affine, Rect};

use crate::{
    canvas::Canvas,
    shared::{
        bounds::{Bounds, PixelBounds, TileBbox},
        brush::{Brush, decode_encoded_brush, push_encoded_brush},
        draw_record::DrawRecord,
        execution::{ExecOp, ExecPlan},
        gpu_sdf::{decode_sdf, decode_sdf_shadow, push_encoded_sdf, push_encoded_sdf_shadow},
        layer::{
            Layer,
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

// Offscreen rendering reuses the main canvas data, but filter kernels and scratch
// images run in a local surface whose origin may be outside the canvas. Keeping the
// coordinate conversion in one type makes it clear which values move into local
// space and which values, such as fixed filter regions, stay in buffer space.
#[derive(Clone, Copy)]
struct LocalSpace {
    surface: Bounds,
}

impl LocalSpace {
    fn new(surface: Bounds) -> Self {
        Self { surface }
    }

    fn line(self, line: Line) -> Line {
        let dx = -self.surface.x0 as f32;
        let dy = -self.surface.y0 as f32;
        Line {
            path_id: line.path_id,
            _pad: line._pad,
            p0: [line.p0[0] + dx, line.p0[1] + dy],
            p1: [line.p1[0] + dx, line.p1[1] + dy],
        }
    }

    fn pixel_bounds(self, bounds: PixelBounds) -> PixelBounds {
        PixelBounds {
            x0: bounds.x0 - self.surface.x0,
            y0: bounds.y0 - self.surface.y0,
            x1: bounds.x1 - self.surface.x0,
            y1: bounds.y1 - self.surface.y0,
        }
    }

    fn bounds(self, bounds: Bounds) -> Bounds {
        Bounds::new(
            bounds.x0 - self.surface.x0,
            bounds.y0 - self.surface.y0,
            bounds.x1 - self.surface.x0,
            bounds.y1 - self.surface.y0,
        )
    }

    fn rect(self, rect: Rect) -> Rect {
        let dx = f64::from(self.surface.x0);
        let dy = f64::from(self.surface.y0);
        Rect::new(rect.x0 - dx, rect.y0 - dy, rect.x1 - dx, rect.y1 - dy)
    }

    fn transform(self, transform: Affine) -> Affine {
        Affine::translate((-f64::from(self.surface.x0), -f64::from(self.surface.y0))) * transform
    }

    fn sdf(self, sdf: Sdf) -> Sdf {
        sdf.translated(self.surface.x0 as f32, self.surface.y0 as f32)
    }

    fn sdf_shadow(self, sdf_shadow: SdfShadow) -> SdfShadow {
        sdf_shadow.translated(self.surface.x0 as f32, self.surface.y0 as f32)
    }

    fn brush(self, brush: Brush) -> Brush {
        let ox = self.surface.x0 as f32;
        let oy = self.surface.y0 as f32;
        match brush {
            Brush::Solid(_) => brush,
            Brush::Linear(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Linear(gradient)
            }
            Brush::Radial(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Radial(gradient)
            }
            Brush::Sweep(mut gradient) => {
                gradient.center[0] -= ox;
                gradient.center[1] -= oy;
                Brush::Sweep(gradient)
            }
            Brush::FourCorner(mut gradient) => {
                gradient.bounds[0] -= ox;
                gradient.bounds[1] -= oy;
                gradient.bounds[2] -= ox;
                gradient.bounds[3] -= oy;
                Brush::FourCorner(gradient)
            }
            Brush::Mesh(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Mesh(gradient)
            }
            Brush::Program(mut program) => {
                program.0.transform = self.brush_transform(program.0.transform);
                Brush::Program(program)
            }
            Brush::Pattern(mut pattern) => {
                pattern.transform = self.brush_transform(pattern.transform);
                Brush::Pattern(pattern)
            }
        }
    }

    fn brush_transform(self, transform: [f32; 6]) -> [f32; 6] {
        let [a, b, c, d, e, f] = transform;
        let ox = self.surface.x0 as f32;
        let oy = self.surface.y0 as f32;
        [a, b, c, d, a * ox + c * oy + e, b * ox + d * oy + f]
    }

    fn turbulence(self, turbulence: Turbulence) -> FilterPrimitiveKind {
        let mut turbulence = turbulence;
        turbulence.transform_x -= self.surface.x0 as f32;
        turbulence.transform_y -= self.surface.y0 as f32;
        turbulence.tile_x -= self.surface.x0 as f32;
        turbulence.tile_y -= self.surface.y0 as f32;
        FilterPrimitiveKind::Turbulence(turbulence)
    }
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
        ops: local_children.clone(),
        layer_stack_data: plan.layer_stack_data.clone(),
        draw_order: Default::default(),
        draw_batch_ids: Default::default(),
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
                owners: owners.clone(),
            },
            ExecOp::BeginClip => ExecOp::BeginClip,
            ExecOp::EndClip => ExecOp::EndClip,
            ExecOp::BeginOpacity => ExecOp::BeginOpacity,
            ExecOp::EndOpacity => ExecOp::EndOpacity,
            ExecOp::BeginBlend => ExecOp::BeginBlend,
            ExecOp::EndBlend => ExecOp::EndBlend,
            ExecOp::OffscreenLayer {
                retained_id,
                draw,
                layer,
                outer_stack,
                children,
            } => ExecOp::OffscreenLayer {
                retained_id: *retained_id,
                draw: layer_draw(layer, *draw).map_or(0, |draw| draw_map[&draw]),
                layer: translate_layer_to_local(layer, local),
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
    let remap = |draw: u32| {
        draw_map
            .get(&(draw as usize))
            .copied()
            .expect("every used local layer-stack draw must be selected") as u32
    };
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

fn translate_layer_to_local(layer: &Layer, local: LocalSpace) -> Layer {
    match layer {
        Layer::Clip | Layer::Isolate | Layer::Opacity(_) | Layer::Blend(_) => layer.clone(),
        Layer::ClipSdf {
            sdf,
            bounds: sdf_bounds,
        } => Layer::ClipSdf {
            sdf: local.sdf(*sdf),
            bounds: local.bounds(*sdf_bounds),
        },
        Layer::Filter {
            filter,
            sample_region,
        } => Layer::Filter {
            filter: translate_filter_to_local(filter, local),
            sample_region: translate_region_to_local(sample_region, local),
        },
        Layer::Backdrop {
            filter,
            sample_region,
        } => Layer::Backdrop {
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

// Filter/backdrop operations have no geometry record; their draw field is unused.
// Do not turn that sentinel into an additional spatial candidate or layer record.
fn layer_draw(layer: &Layer, draw: usize) -> Option<usize> {
    match layer {
        Layer::Filter { .. } | Layer::Backdrop { .. } => None,
        _ => Some(draw),
    }
}
