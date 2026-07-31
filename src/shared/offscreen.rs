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

struct LocalDrawSelection {
    draws: Vec<usize>,
    batch_draws: HashMap<u32, Vec<usize>>,
}

fn local_draw_selection(
    canvas: &Canvas,
    plan: &ExecPlan,
    children: &[ExecOp],
    candidates: &[u32],
    bounds: Bounds,
) -> LocalDrawSelection {
    let mut batches = HashSet::new();
    let mut hidden = HashSet::new();
    collect_local_plan_draws(children, &plan.layer_stack_data, &mut batches, &mut hidden);
    let mut draws = Vec::new();
    let mut seen = HashSet::new();
    let mut old_batch_draws = HashMap::<u32, Vec<usize>>::new();
    for &draw in candidates {
        let draw = draw as usize;
        let Some(record) = canvas.draw_records.get(draw) else {
            continue;
        };
        let batch = plan.draw_batch_ids.get(draw).copied().unwrap_or(u32::MAX);
        if batches.contains(&batch)
            && pixel_bounds_overlap(record.pixel_bounds, bounds)
            && seen.insert(draw)
        {
            draws.push(draw);
            old_batch_draws.entry(batch).or_default().push(draw);
        }
    }
    for draw in hidden {
        if draw < canvas.draw_records.len() && seen.insert(draw) {
            draws.push(draw);
        }
    }
    let draw_map = draws
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect::<HashMap<_, _>>();
    let batch_draws = old_batch_draws
        .into_iter()
        .map(|(batch, draws)| {
            (
                batch,
                draws.into_iter().map(|draw| draw_map[&draw]).collect(),
            )
        })
        .collect();
    LocalDrawSelection { draws, batch_draws }
}

fn collect_local_plan_draws(
    ops: &[ExecOp],
    layer_stacks: &[crate::shared::execution::LayerStackEntry],
    batches: &mut HashSet<u32>,
    hidden: &mut HashSet<usize>,
) {
    for op in ops {
        match op {
            ExecOp::DrawBatch {
                batch_id,
                layer_stack,
                ..
            } => {
                batches.insert(*batch_id);
                collect_layer_stack_draws(layer_stacks, layer_stack.clone(), hidden);
            }
            ExecOp::OffscreenLayer {
                draw,
                outer_stack,
                children,
                ..
            } => {
                hidden.insert(*draw);
                collect_layer_stack_draws(layer_stacks, outer_stack.clone(), hidden);
                collect_local_plan_draws(children, layer_stacks, batches, hidden);
            }
            ExecOp::OffscreenMaskLayer {
                outer_stack,
                content,
                mask,
                ..
            } => {
                collect_layer_stack_draws(layer_stacks, outer_stack.clone(), hidden);
                collect_local_plan_draws(content, layer_stacks, batches, hidden);
                collect_local_plan_draws(mask, layer_stacks, batches, hidden);
            }
            ExecOp::BeginClip
            | ExecOp::EndClip
            | ExecOp::BeginOpacity
            | ExecOp::EndOpacity
            | ExecOp::BeginBlend
            | ExecOp::EndBlend => {}
        }
    }
}

fn collect_layer_stack_draws(
    entries: &[crate::shared::execution::LayerStackEntry],
    range: std::ops::Range<usize>,
    draws: &mut HashSet<usize>,
) {
    for entry in entries.get(range).unwrap_or_default() {
        let draw = match entry {
            crate::shared::execution::LayerStackEntry::Clip { draw }
            | crate::shared::execution::LayerStackEntry::Opacity { draw, .. }
            | crate::shared::execution::LayerStackEntry::Blend { draw, .. } => *draw,
        };
        draws.insert(draw as usize);
    }
}

fn pixel_bounds_overlap(bounds: PixelBounds, region: Bounds) -> bool {
    bounds.x0 < region.x1 && bounds.x1 > region.x0 && bounds.y0 < region.y1 && bounds.y1 > region.y0
}

fn translated_scene_for_draws(
    canvas: &Canvas,
    local: LocalSpace,
    draws: &[usize],
) -> (Canvas, HashMap<usize, usize>) {
    let mut translated = Canvas::new(local.surface.width(), local.surface.height(), 1.0);
    translated.scene_images.extend_from(&canvas.scene_images);
    let mut draw_map = HashMap::with_capacity(draws.len());
    let mut path_map = HashMap::<u32, u32>::new();
    let mut run_map = HashMap::<u32, u32>::new();
    for &old_draw in draws {
        let Some(mut draw) = canvas.draw_records.get(old_draw).copied() else {
            continue;
        };
        draw.pixel_bounds = local.pixel_bounds(draw.pixel_bounds);
        if let Some(brush) =
            decode_encoded_brush(&canvas.brush_blob, draw.brush_offset, draw.brush_len)
        {
            (draw.brush_offset, draw.brush_len) =
                push_encoded_brush(&mut translated.brush_blob, &local.brush(brush));
        } else {
            draw.brush_offset = DrawRecord::NONE;
            draw.brush_len = 0;
        }
        if let Some(sdf) = decode_sdf(&canvas.sdf_blob, draw.sdf_offset, draw.sdf_len) {
            (draw.sdf_offset, draw.sdf_len) =
                push_encoded_sdf(&mut translated.sdf_blob, local.sdf(sdf));
            draw.sdf_shadow_offset = DrawRecord::NONE;
            draw.sdf_shadow_len = 0;
        } else if let Some(sdf_shadow) = decode_sdf_shadow(
            &canvas.sdf_shadow_blob,
            draw.sdf_shadow_offset,
            draw.sdf_shadow_len,
        ) {
            (draw.sdf_shadow_offset, draw.sdf_shadow_len) = push_encoded_sdf_shadow(
                &mut translated.sdf_shadow_blob,
                local.sdf_shadow(sdf_shadow),
            );
            draw.sdf_offset = DrawRecord::NONE;
            draw.sdf_len = 0;
        } else {
            draw.sdf_offset = DrawRecord::NONE;
            draw.sdf_len = 0;
            draw.sdf_shadow_offset = DrawRecord::NONE;
            draw.sdf_shadow_len = 0;
        }
        if let Some(old_path) = draw.path_id() {
            draw.path_id = *path_map.entry(old_path).or_insert_with(|| {
                let old = canvas.path_records[old_path as usize];
                let new = translated.path_records.len() as u32;
                let line_start = translated.lines.len() as u32;
                translated.lines.extend(
                    canvas.lines
                        [old.line_start as usize..(old.line_start + old.line_count) as usize]
                        .iter()
                        .copied()
                        .map(|line| {
                            let mut line = local.line(line);
                            line.path_id = new;
                            line
                        }),
                );
                translated.path_records.push(PathRecord {
                    path_id: new,
                    line_start,
                    ..old
                });
                new
            });
        }
        if let Some(old_run) = draw.glyph_run_id() {
            draw.glyph_run_id = *run_map.entry(old_run).or_insert_with(|| {
                let old = canvas.text_runs[old_run as usize];
                let new = translated.text_runs.len() as u32;
                let glyph_start = translated.text_glyphs.len() as u32;
                translated.text_glyphs.extend(
                    canvas.text_glyphs
                        [old.glyph_start as usize..(old.glyph_start + old.glyph_count) as usize]
                        .iter()
                        .copied()
                        .map(|glyph| {
                            glyph.translated(
                                -f64::from(local.surface.x0),
                                -f64::from(local.surface.y0),
                            )
                        }),
                );
                translated.text_runs.push(crate::text::TextRun {
                    glyph_start,
                    glyph_count: old.glyph_count,
                });
                new
            });
        }
        draw_map.insert(old_draw, translated.draw_records.len());
        translated.draw_records.push(draw);
    }
    rebuild_translated_path_backdrops(&mut translated);
    translated.path_cnt = translated.path_records.len() as u32;
    translated.backdrop_pool_capacity = translated
        .path_records
        .last()
        .map(|record| record.data_offset + record.data_len)
        .unwrap_or(0);
    translated.tile_cnt = translated
        .path_records
        .last()
        .map(|record| record.segment_start + record.segment_capacity)
        .unwrap_or(0);
    (translated, draw_map)
}

fn rebuild_translated_path_backdrops(canvas: &mut Canvas) {
    let mut path_bounds: Vec<Option<PixelBounds>> = vec![None; canvas.path_records.len()];
    for draw in &canvas.draw_records {
        if let Some(path_id) = draw.path_id()
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
    let width_in_tiles = canvas.width_in_tiles();
    let height_in_tiles = canvas.height_in_tiles();
    for path_id in 0..canvas.path_records.len() {
        let pixel_bounds = path_bounds
            .get(path_id)
            .and_then(|bounds| *bounds)
            .unwrap_or_else(|| translated_path_pixel_bounds(canvas, path_id));
        let tile_bbox = pixel_bounds.tile_bbox(width_in_tiles, height_in_tiles);
        let data_len = tile_bbox.tile_count();
        let segment_capacity = translated_path_segment_capacity(canvas, path_id, tile_bbox);
        let record = &mut canvas.path_records[path_id];
        *record = PathRecord {
            data_offset,
            data_len,
            tile_x0: tile_bbox.x0,
            tile_y0: tile_bbox.y0,
            tile_x1: tile_bbox.x1,
            tile_y1: tile_bbox.y1,
            segment_start,
            segment_capacity,
            segment_count: 0,
            ..*record
        };
        data_offset += data_len;
        segment_start += segment_capacity;
    }
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
    lines
        .iter()
        .map(|line| {
            crate::shared::scan_line::line_scanned_tile_count(
                *line,
                tile_bbox,
                (canvas.width_in_tiles(), canvas.height_in_tiles()),
            )
        })
        .sum()
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
                draws,
                batch_id,
                layer_stack,
                owners,
            } => ExecOp::DrawBatch {
                draws: std::rc::Rc::new(batch_draws.get(batch_id).cloned().unwrap_or_else(|| {
                    draws
                        .iter()
                        .filter_map(|draw| draw_map.get(draw).copied())
                        .collect()
                })),
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
                draw: draw_map[draw],
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
    use crate::shared::{
        execution::ROOT_COMMAND_LIST_ID, fill::FillRule, image::Image, image_resource::ImageKey,
    };
    use peniko::{Color, kurbo::Shape};

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

    #[test]
    fn translated_scene_preserves_scene_image_resources() {
        let key = ImageKey::new(42);
        let mut canvas = Canvas::new(8, 8, 1.0);
        assert!(
            canvas
                .scene_images
                .insert(key, Image::from_rgba8(1, 1, [255, 0, 0, 255]))
        );

        let (translated, _) =
            translated_scene_for_draws(&canvas, LocalSpace::new(Bounds::new(2, 2, 6, 6)), &[]);

        let image = translated
            .scene_image_resources()
            .get(key)
            .expect("translated scene should keep scene image resources");
        assert_eq!(image.rgba8_at(0, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn local_scene_compacts_draw_and_path_ids_to_the_filter_region() {
        let mut canvas = Canvas::new(128, 128, 1.0);
        canvas.push_path(
            Rect::new(12.0, 12.0, 28.0, 28.0).to_path(0.1),
            Color::from_rgb8(255, 0, 0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.1,
        );
        canvas.push_path(
            Rect::new(96.0, 96.0, 112.0, 112.0).to_path(0.1),
            Color::from_rgb8(0, 0, 255),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.1,
        );
        let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
        let local = local_offscreen_scene(
            &canvas,
            &plan,
            &plan.ops,
            Bounds::new(8, 8, 40, 40),
            &[0, 1],
        );

        assert_eq!(local.canvas.draw_records.len(), 1);
        assert_eq!(local.canvas.path_records.len(), 1);
        assert_eq!(local.canvas.draw_records[0].path_id, 0);
        assert_eq!(local.canvas.path_records[0].path_id, 0);
        assert!(local.canvas.lines.iter().all(|line| line.path_id == 0));
        assert_eq!(local.canvas.draw_records[0].pixel_bounds.x0, 4);
        let ExecOp::DrawBatch { draws, .. } = &local.children[0] else {
            panic!("plain paths should remain one local draw batch")
        };
        assert_eq!(draws.as_slice(), &[0]);
    }
}
