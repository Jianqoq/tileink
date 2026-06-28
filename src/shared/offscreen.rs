use peniko::kurbo::Affine;

use crate::{
    scene::Scene,
    shared::{
        bd_record::BackdropRecord,
        bounds::{Bounds, PixelBounds, TileBbox},
        brush::Brush,
        execution::{ExecOp, ExecPlan},
        layer::{
            Layer,
            filter::{Filter, FilterPrimitive, FilterPrimitiveKind},
            mask::Mask,
            region::Region,
        },
        line::Line,
        sdf::Sdf,
    },
};

pub(crate) struct LocalOffscreenScene {
    pub(crate) scene: Scene,
    pub(crate) plan: ExecPlan,
    pub(crate) children: Vec<ExecOp>,
}

pub(crate) fn local_offscreen_scene(
    scene: &Scene,
    plan: &ExecPlan,
    children: &[ExecOp],
    bounds: Bounds,
    line_scanned_tile_count: impl Fn(Line, TileBbox, (u32, u32)) -> u32,
) -> LocalOffscreenScene {
    let local_children = translate_exec_ops_to_local(children, bounds);
    LocalOffscreenScene {
        scene: translated_scene_for_bounds(scene, bounds, line_scanned_tile_count),
        plan: ExecPlan {
            ops: local_children.clone(),
            layer_stack_data: plan.layer_stack_data.clone(),
        },
        children: local_children,
    }
}

pub(crate) fn local_filter(filter: &Filter, bounds: Bounds) -> Filter {
    translate_filter_to_local(filter, bounds)
}

fn translated_scene_for_bounds(
    scene: &Scene,
    bounds: Bounds,
    line_scanned_tile_count: impl Fn(Line, TileBbox, (u32, u32)) -> u32,
) -> Scene {
    let dx = -bounds.x0 as f32;
    let dy = -bounds.y0 as f32;
    let mut translated = Scene::new(bounds.width(), bounds.height());
    translated.lines = scene
        .lines
        .iter()
        .map(|line| Line {
            path_id: line.path_id,
            _pad: line._pad,
            p0: [line.p0[0] + dx, line.p0[1] + dy],
            p1: [line.p1[0] + dx, line.p1[1] + dy],
        })
        .collect();
    translated.path_records = scene.path_records.clone();
    translated.draw_records = scene
        .draw_records
        .iter()
        .map(|draw| {
            let mut draw = draw.clone();
            draw.pixel_bounds = shift_pixel_bounds(draw.pixel_bounds, -bounds.x0, -bounds.y0);
            draw.sdf = draw.sdf.map(|sdf| translate_sdf_to_local(sdf, bounds));
            draw.brush = translate_brush_to_local(draw.brush, bounds);
            draw
        })
        .collect();
    translated.bd_records =
        translated_backdrop_records(scene, &translated, line_scanned_tile_count);
    translated.path_cnt = scene.path_cnt;
    translated.backdrop_pool_capacity = translated
        .bd_records
        .last()
        .map(|record| record.data_offset + record.data_len)
        .unwrap_or(0);
    translated.tile_cnt = translated
        .bd_records
        .last()
        .map(|record| record.segment_start + record.segment_capacity)
        .unwrap_or(0);
    translated
}

fn translated_backdrop_records(
    scene: &Scene,
    translated: &Scene,
    line_scanned_tile_count: impl Fn(Line, TileBbox, (u32, u32)) -> u32,
) -> Vec<BackdropRecord> {
    let mut path_bounds: Vec<Option<PixelBounds>> = vec![None; translated.path_records.len()];
    for draw in &translated.draw_records {
        if let Some(path_id) = draw.path_id
            && let Some(slot) = path_bounds.get_mut(path_id as usize)
        {
            *slot = Some(match *slot {
                Some(bounds) => bounds.union(draw.pixel_bounds),
                None => draw.pixel_bounds,
            });
        }
    }

    let mut data_offset = 0;
    let mut segment_start = 0;
    let width_in_tiles = translated.width_in_tiles();
    let height_in_tiles = translated.height_in_tiles();
    scene
        .bd_records
        .iter()
        .map(|record| {
            let path_id = record.path_id as usize;
            let pixel_bounds = path_bounds
                .get(path_id)
                .and_then(|bounds| *bounds)
                .unwrap_or_else(|| translated_path_pixel_bounds(translated, path_id));
            let tile_bbox = pixel_bounds.tile_bbox(width_in_tiles, height_in_tiles);
            let data_len = tile_bbox.tile_count();
            let segment_capacity = translated_path_segment_capacity(
                translated,
                path_id,
                tile_bbox,
                &line_scanned_tile_count,
            );
            let translated_record = BackdropRecord {
                path_id: record.path_id,
                data_offset,
                data_len,
                tile_x0: tile_bbox.x0,
                tile_y0: tile_bbox.y0,
                tile_x1: tile_bbox.x1,
                tile_y1: tile_bbox.y1,
                segment_start,
                segment_capacity,
                segment_count: 0,
            };
            data_offset += data_len;
            segment_start += segment_capacity;
            translated_record
        })
        .collect()
}

fn translated_path_pixel_bounds(scene: &Scene, path_id: usize) -> PixelBounds {
    let Some(record) = scene.path_records.get(path_id) else {
        return empty_pixel_bounds();
    };
    let lines =
        &scene.lines[record.line_start as usize..(record.line_start + record.line_count) as usize];
    if lines.is_empty() {
        return empty_pixel_bounds();
    }

    let mut x0 = f32::INFINITY;
    let mut y0 = f32::INFINITY;
    let mut x1 = f32::NEG_INFINITY;
    let mut y1 = f32::NEG_INFINITY;
    for line in lines {
        x0 = x0.min(line.p0[0]).min(line.p1[0]);
        y0 = y0.min(line.p0[1]).min(line.p1[1]);
        x1 = x1.max(line.p0[0]).max(line.p1[0]);
        y1 = y1.max(line.p0[1]).max(line.p1[1]);
    }
    PixelBounds {
        x0: x0.floor() as i32,
        y0: y0.floor() as i32,
        x1: x1.ceil() as i32,
        y1: y1.ceil() as i32,
    }
}

fn empty_pixel_bounds() -> PixelBounds {
    PixelBounds {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    }
}

fn translated_path_segment_capacity(
    scene: &Scene,
    path_id: usize,
    tile_bbox: TileBbox,
    line_scanned_tile_count: &impl Fn(Line, TileBbox, (u32, u32)) -> u32,
) -> u32 {
    let Some(record) = scene.path_records.get(path_id) else {
        return 0;
    };
    let lines =
        &scene.lines[record.line_start as usize..(record.line_start + record.line_count) as usize];
    lines
        .iter()
        .map(|line| {
            line_scanned_tile_count(
                *line,
                tile_bbox,
                (scene.width_in_tiles(), scene.height_in_tiles()),
            )
        })
        .sum()
}

fn translate_exec_ops_to_local(ops: &[ExecOp], bounds: Bounds) -> Vec<ExecOp> {
    ops.iter()
        .map(|op| match op {
            ExecOp::DrawBatch { draws, layer_stack } => ExecOp::DrawBatch {
                draws: draws.clone(),
                layer_stack: layer_stack.clone(),
            },
            ExecOp::BeginClip => ExecOp::BeginClip,
            ExecOp::EndClip => ExecOp::EndClip,
            ExecOp::BeginOpacity => ExecOp::BeginOpacity,
            ExecOp::EndOpacity => ExecOp::EndOpacity,
            ExecOp::BeginBlend => ExecOp::BeginBlend,
            ExecOp::EndBlend => ExecOp::EndBlend,
            ExecOp::OffscreenLayer {
                draw,
                layer,
                outer_stack,
                children,
            } => ExecOp::OffscreenLayer {
                draw: *draw,
                layer: translate_layer_to_local(layer, bounds),
                outer_stack: outer_stack.clone(),
                children: translate_exec_ops_to_local(children, bounds),
            },
            ExecOp::OffscreenMaskLayer {
                layer,
                outer_stack,
                content,
                mask,
            } => ExecOp::OffscreenMaskLayer {
                layer: translate_mask_to_local(layer, bounds),
                outer_stack: outer_stack.clone(),
                content: translate_exec_ops_to_local(content, bounds),
                mask: translate_exec_ops_to_local(mask, bounds),
            },
        })
        .collect()
}

fn translate_layer_to_local(layer: &Layer, bounds: Bounds) -> Layer {
    match layer {
        Layer::Clip | Layer::Isolate | Layer::Opacity(_) | Layer::Blend(_) => layer.clone(),
        Layer::ClipSdf {
            sdf,
            bounds: sdf_bounds,
        } => Layer::ClipSdf {
            sdf: translate_sdf_to_local(*sdf, bounds),
            bounds: shift_bounds(*sdf_bounds, -bounds.x0, -bounds.y0),
        },
        Layer::Filter {
            filter,
            sample_region,
        } => Layer::Filter {
            filter: translate_filter_to_local(filter, bounds),
            sample_region: translate_region_to_local(sample_region, bounds),
        },
        Layer::Backdrop {
            filter,
            sample_region,
        } => Layer::Backdrop {
            filter: translate_filter_to_local(filter, bounds),
            sample_region: translate_region_to_local(sample_region, bounds),
        },
    }
}

fn translate_mask_to_local(mask: &Mask, bounds: Bounds) -> Mask {
    Mask {
        region: translate_region_to_local(&mask.region, bounds),
        kind: mask.kind,
    }
}

fn translate_filter_to_local(filter: &Filter, bounds: Bounds) -> Filter {
    match filter {
        Filter::Chain {
            filters,
            fixed_region,
        } => Filter::Chain {
            filters: filters
                .iter()
                .map(|filter| translate_filter_to_local(filter, bounds))
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
                    region: shift_bounds(primitive.region, -bounds.x0, -bounds.y0),
                    kind: translate_primitive_kind_to_local(&primitive.kind, bounds),
                })
                .collect(),
            fixed_region: *fixed_region,
        },
        Filter::Flood { brush } => Filter::Flood {
            brush: translate_brush_to_local(brush.clone(), bounds),
        },
        Filter::DropShadow {
            offset_x,
            offset_y,
            radius,
            brush,
        } => Filter::DropShadow {
            offset_x: *offset_x,
            offset_y: *offset_y,
            radius: *radius,
            brush: translate_brush_to_local(brush.clone(), bounds),
        },
        _ => filter.clone(),
    }
}

fn translate_primitive_kind_to_local(
    kind: &FilterPrimitiveKind,
    bounds: Bounds,
) -> FilterPrimitiveKind {
    match kind {
        FilterPrimitiveKind::Filter(filter) => {
            FilterPrimitiveKind::Filter(Box::new(translate_filter_to_local(filter, bounds)))
        }
        FilterPrimitiveKind::Image { brush } => FilterPrimitiveKind::Image {
            brush: translate_brush_to_local(brush.clone(), bounds),
        },
        FilterPrimitiveKind::Tile { source_region } => FilterPrimitiveKind::Tile {
            source_region: shift_bounds(*source_region, -bounds.x0, -bounds.y0),
        },
        _ => kind.clone(),
    }
}

fn translate_region_to_local(region: &Region, bounds: Bounds) -> Region {
    match region {
        Region::Rect { rect, radius } => Region::rect(
            peniko::kurbo::Rect::new(
                rect.x0 - f64::from(bounds.x0),
                rect.y0 - f64::from(bounds.y0),
                rect.x1 - f64::from(bounds.x0),
                rect.y1 - f64::from(bounds.y0),
            ),
            *radius,
        ),
        Region::Path {
            path,
            transform,
            tolerance,
        } => Region::path(
            path.clone(),
            Affine::translate((-f64::from(bounds.x0), -f64::from(bounds.y0))) * *transform,
            *tolerance,
        ),
    }
}

fn translate_sdf_to_local(sdf: Sdf, bounds: Bounds) -> Sdf {
    let dx = f64::from(bounds.x0);
    let dy = f64::from(bounds.y0);
    match sdf {
        Sdf::Rect(mut rect) => {
            rect.start.x -= dx;
            rect.start.y -= dy;
            rect.end.x -= dx;
            rect.end.y -= dy;
            Sdf::Rect(rect)
        }
        Sdf::RectStroke(mut stroke) => {
            stroke.rect.start.x -= dx;
            stroke.rect.start.y -= dy;
            stroke.rect.end.x -= dx;
            stroke.rect.end.y -= dy;
            Sdf::RectStroke(stroke)
        }
        Sdf::Circle(mut circle) => {
            circle.center.x -= dx;
            circle.center.y -= dy;
            Sdf::Circle(circle)
        }
        Sdf::CircleStroke(mut stroke) => {
            stroke.circle.center.x -= dx;
            stroke.circle.center.y -= dy;
            Sdf::CircleStroke(stroke)
        }
    }
}

fn translate_brush_to_local(brush: Brush, bounds: Bounds) -> Brush {
    let ox = bounds.x0 as f32;
    let oy = bounds.y0 as f32;
    match brush {
        Brush::Solid(_) => brush,
        Brush::Linear(mut gradient) => {
            gradient.transform = pretranslate_brush_transform(gradient.transform, ox, oy);
            Brush::Linear(gradient)
        }
        Brush::Radial(mut gradient) => {
            gradient.transform = pretranslate_brush_transform(gradient.transform, ox, oy);
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
        Brush::Pattern(mut pattern) => {
            pattern.transform = pretranslate_brush_transform(pattern.transform, ox, oy);
            Brush::Pattern(pattern)
        }
    }
}

fn pretranslate_brush_transform(transform: [f32; 6], ox: f32, oy: f32) -> [f32; 6] {
    let [a, b, c, d, e, f] = transform;
    [a, b, c, d, a * ox + c * oy + e, b * ox + d * oy + f]
}

fn shift_pixel_bounds(bounds: PixelBounds, dx: i32, dy: i32) -> PixelBounds {
    PixelBounds {
        x0: bounds.x0 + dx,
        y0: bounds.y0 + dy,
        x1: bounds.x1 + dx,
        y1: bounds.y1 + dy,
    }
}

fn shift_bounds(bounds: Bounds, dx: i32, dy: i32) -> Bounds {
    Bounds::new(
        bounds.x0 + dx,
        bounds.y0 + dy,
        bounds.x1 + dx,
        bounds.y1 + dy,
    )
}
