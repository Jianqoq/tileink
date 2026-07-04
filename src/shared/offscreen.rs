use peniko::kurbo::{Affine, Rect};

use crate::{
    canvas::Canvas,
    shared::{
        bd_record::BackdropRecord,
        bounds::{Bounds, PixelBounds, TileBbox},
        brush::Brush,
        execution::{ExecOp, ExecPlan},
        layer::{
            Layer,
            filter::{Filter, FilterPrimitive, FilterPrimitiveKind, Turbulence},
            mask::Mask,
            region::Region,
        },
        line::Line,
        path::PATH_FLAG_KEEP_HORIZONTAL_TILE_EDGES,
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
) -> LocalOffscreenCanvas {
    let local = LocalSpace::new(bounds);
    let local_children = translate_exec_ops_to_local(children, local);
    LocalOffscreenCanvas {
        canvas: translated_scene_for_bounds(canvas, local),
        plan: ExecPlan {
            ops: local_children.clone(),
            layer_stack_data: plan.layer_stack_data.clone(),
        },
        children: local_children,
    }
}

pub(crate) fn local_filter(filter: &Filter, bounds: Bounds) -> Filter {
    translate_filter_to_local(filter, LocalSpace::new(bounds))
}

fn translated_scene_for_bounds(canvas: &Canvas, local: LocalSpace) -> Canvas {
    let mut translated = Canvas::new(local.surface.width(), local.surface.height());
    translated.lines = canvas
        .lines
        .iter()
        .copied()
        .map(|line| local.line(line))
        .collect();
    translated.path_records = canvas.path_records.clone();
    translated.draw_records = canvas
        .draw_records
        .iter()
        .map(|draw| {
            let mut draw = draw.clone();
            draw.pixel_bounds = local.pixel_bounds(draw.pixel_bounds);
            draw
        })
        .collect();
    translated.brushes = canvas
        .brushes
        .iter()
        .cloned()
        .map(|brush| local.brush(brush))
        .collect();
    translated.sdfs = canvas
        .sdfs
        .iter()
        .copied()
        .map(|sdf| local.sdf(sdf))
        .collect();
    translated.sdf_shadows = canvas
        .sdf_shadows
        .iter()
        .copied()
        .map(|sdf_shadow| local.sdf_shadow(sdf_shadow))
        .collect();
    translated.text_glyphs = canvas
        .text_glyphs
        .iter()
        .copied()
        .map(|glyph| glyph.translated(-f64::from(local.surface.x0), -f64::from(local.surface.y0)))
        .collect();
    translated.text_runs = canvas.text_runs.clone();
    translated.bd_records = translated_backdrop_records(canvas, &translated);
    translated.path_cnt = canvas.path_cnt;
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

fn translated_backdrop_records(canvas: &Canvas, translated: &Canvas) -> Vec<BackdropRecord> {
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
    canvas
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
            let segment_capacity = translated_path_segment_capacity(translated, path_id, tile_bbox);
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

fn translated_path_pixel_bounds(canvas: &Canvas, path_id: usize) -> PixelBounds {
    let Some(record) = canvas.path_records.get(path_id) else {
        return empty_pixel_bounds();
    };
    let lines =
        &canvas.lines[record.line_start as usize..(record.line_start + record.line_count) as usize];
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

fn translated_path_segment_capacity(canvas: &Canvas, path_id: usize, tile_bbox: TileBbox) -> u32 {
    let Some(record) = canvas.path_records.get(path_id) else {
        return 0;
    };
    let lines =
        &canvas.lines[record.line_start as usize..(record.line_start + record.line_count) as usize];
    let keep_horizontal_tile_edges = record.flags & PATH_FLAG_KEEP_HORIZONTAL_TILE_EDGES != 0;
    lines
        .iter()
        .map(|line| {
            crate::shared::scan_line::line_scanned_tile_count(
                *line,
                tile_bbox,
                (canvas.width_in_tiles(), canvas.height_in_tiles()),
                keep_horizontal_tile_edges,
            )
        })
        .sum()
}

fn translate_exec_ops_to_local(ops: &[ExecOp], local: LocalSpace) -> Vec<ExecOp> {
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
                layer: translate_layer_to_local(layer, local),
                outer_stack: outer_stack.clone(),
                children: translate_exec_ops_to_local(children, local),
            },
            ExecOp::OffscreenMaskLayer {
                layer,
                outer_stack,
                content,
                mask,
            } => ExecOp::OffscreenMaskLayer {
                layer: translate_mask_to_local(layer, local),
                outer_stack: outer_stack.clone(),
                content: translate_exec_ops_to_local(content, local),
                mask: translate_exec_ops_to_local(mask, local),
            },
        })
        .collect()
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
                    kind: translate_primitive_kind_to_local(&primitive.kind, local),
                })
                .collect(),
            fixed_region: *fixed_region,
        },
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
mod tests {
    use super::*;

    #[test]
    fn local_space_shifts_geometry_to_surface_origin() {
        let local = LocalSpace::new(Bounds::new(10, 20, 30, 40));

        let line = local.line(Line {
            path_id: 3,
            _pad: 0.0,
            p0: [12.0, 23.0],
            p1: [18.0, 31.0],
        });
        assert_eq!(line.path_id, 3);
        assert_eq!(line.p0, [2.0, 3.0]);
        assert_eq!(line.p1, [8.0, 11.0]);
        assert_eq!(
            local.pixel_bounds(PixelBounds {
                x0: 11,
                y0: 22,
                x1: 29,
                y1: 39,
            }),
            PixelBounds {
                x0: 1,
                y0: 2,
                x1: 19,
                y1: 19,
            }
        );
        assert_eq!(
            local.bounds(Bounds::new(12, 24, 25, 36)),
            Bounds::new(2, 4, 15, 16)
        );
    }

    #[test]
    fn local_space_keeps_brush_sampling_in_world_space() {
        let local = LocalSpace::new(Bounds::new(10, 20, 30, 40));

        assert_eq!(
            local.brush_transform([2.0, 3.0, 5.0, 7.0, 11.0, 13.0]),
            [2.0, 3.0, 5.0, 7.0, 131.0, 183.0]
        );
    }
}
