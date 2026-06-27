use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use peniko::{
    Color,
    kurbo::{Point, Shape},
};

use crate::{
    cpu::{
        computes::blend::composite_src_over_masked_at,
        computes::fine::{build_tile_alpha, combine_alpha},
        pipelines::{
            coarse::CoarseCpuPipeline, cumsum::CumsumCpuPipeline, filter::FilterCpuPipeline,
            fine::FineCpuPipeline, scan::ScanCpuPipeline,
        },
    },
    render::Render,
    shared::{
        bounds::Bounds,
        draw_record::DrawRecord,
        execution::{ExecOp, ExecPlan, LayerStackEntry},
        image::{Image, rgba8_pack},
        layer::Layer,
        layer::region::Region,
        line_seg::LineSegment,
        pixel::coverage_f32_to_u8,
        sdf::{Sdf, rect::Rect as SdfRect},
        tile_ptcl::{TilePtcl, TilePtclRange},
        tile_seg_range::TileSegmentRange,
    },
};

pub struct Renderer {
    image: Image,
    clear: Color,
    scan: ScanCpuPipeline,
    cumsum: CumsumCpuPipeline,
    coarse: CoarseCpuPipeline,
    fine: FineCpuPipeline,
    filter: FilterCpuPipeline,
    size: (u32, u32),

    backdrops: Vec<i32>,
    tile_segment_ranges: Vec<TileSegmentRange>,
    segments: Vec<LineSegment>,
    segments_bump: Vec<AtomicU32>,
    segment_tile_counts: Vec<u32>,
    segment_tile_cursors: Vec<AtomicU32>,
    tile_ptcl_ranges: Vec<TilePtclRange>,
    tile_ptcls: Vec<TilePtcl>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderProfile {
    pub scan: Duration,
    pub cumsum: Duration,
    pub coarse: Duration,
    pub fine: Duration,
    pub total: Duration,
}

struct OffscreenLayerRef<'a> {
    layer: &'a Layer,
    outer_stack: std::ops::Range<usize>,
    children: &'a [ExecOp],
}

impl Render for Renderer {
    type ScanArgs<'a> = ();

    type CumsumArgs<'a> = ();

    type CoarseArgs<'a> = (
        &'a [DrawRecord],
        std::ops::Range<usize>,
        &'a [LayerStackEntry],
        std::ops::Range<usize>,
    );

    type FineArgs<'a> = (&'a mut Image, Bounds);

    type ExecuteArgs<'a> = &'a mut Image;

    fn render(&mut self, scene: &crate::scene::Scene) {
        let mut image = Image::new(self.size.0, self.size.1, self.clear);
        self.execute(scene, &mut image);
        self.image = image;
    }

    fn execute(&mut self, scene: &crate::scene::Scene, args: Self::ExecuteArgs<'_>) {
        let plan = scene.compile(0);
        self.scan(scene, ());
        self.cumsum(scene, ());
        self.execute_plan(scene, &plan, args);
    }

    fn scan(&mut self, scene: &crate::scene::Scene, _: Self::ScanArgs<'_>) {
        let Some(last_bd_record) = scene.bd_records.last().copied() else {
            self.backdrops.clear();
            self.tile_segment_ranges.clear();
            self.segments.clear();
            self.segment_tile_counts.clear();
            self.segment_tile_cursors.clear();
            self.segments_bump.clear();
            return;
        };
        self.backdrops.resize(
            last_bd_record.data_offset as usize + last_bd_record.data_len as usize,
            0,
        );
        self.tile_segment_ranges.resize(
            last_bd_record.data_offset as usize + last_bd_record.data_len as usize,
            TileSegmentRange::default(),
        );
        self.segments.resize(
            last_bd_record.segment_start as usize + last_bd_record.segment_capacity as usize,
            LineSegment::default(),
        );
        self.segment_tile_counts.resize(
            last_bd_record.data_offset as usize + last_bd_record.data_len as usize,
            0,
        );
        self.segment_tile_cursors.resize_with(
            last_bd_record.data_offset as usize + last_bd_record.data_len as usize,
            || AtomicU32::new(0),
        );
        self.segments_bump
            .resize_with(scene.bd_records.len(), || AtomicU32::new(0));
        for bump in &self.segments_bump {
            bump.store(0, Ordering::Relaxed);
        }
        self.scan
            .prepare(
                &scene.lines,
                &scene.path_records,
                &scene.bd_records,
                &mut self.backdrops,
                &mut self.tile_segment_ranges,
                &mut self.segments,
                &mut self.segments_bump,
                &mut self.segment_tile_counts,
                &mut self.segment_tile_cursors,
                (scene.width_in_tiles(), scene.height_in_tiles()),
            )
            .run();
    }

    fn cumsum(&mut self, scene: &crate::scene::Scene, _: Self::CumsumArgs<'_>) {
        self.cumsum
            .prepare(&mut self.backdrops, &scene.bd_records)
            .run();
    }

    fn coarse(&mut self, scene: &crate::scene::Scene, args: Self::CoarseArgs<'_>) {
        let (draw_records, draw_range, layer_stack_data, layer_stack_range) = args;
        self.coarse
            .prepare(
                draw_records,
                draw_range,
                layer_stack_data,
                layer_stack_range,
                &scene.bd_records,
                &self.backdrops,
                &self.tile_segment_ranges,
                &mut self.tile_ptcl_ranges,
                &mut self.tile_ptcls,
                (scene.width_in_tiles(), scene.height_in_tiles()),
            )
            .run();
    }

    fn fine(&mut self, scene: &crate::scene::Scene, target: Self::FineArgs<'_>) {
        let (target, target_bounds) = target;
        self.fine
            .prepare(
                &self.tile_ptcl_ranges,
                &self.tile_ptcls,
                &self.segments,
                target,
                target_bounds,
                (scene.width_in_tiles(), scene.height_in_tiles()),
            )
            .run();
    }
}

impl Renderer {
    pub fn new(width: u32, height: u32, clear: Color) -> Self {
        Self {
            image: Image::new(width, height, clear),
            clear,
            scan: ScanCpuPipeline::new(),
            cumsum: CumsumCpuPipeline::new(),
            coarse: CoarseCpuPipeline::new(),
            fine: FineCpuPipeline::new(),
            filter: FilterCpuPipeline::new(),
            size: (width, height),
            backdrops: Vec::new(),
            tile_segment_ranges: Vec::new(),
            segments: Vec::new(),
            segments_bump: Vec::new(),
            segment_tile_counts: Vec::new(),
            segment_tile_cursors: Vec::new(),
            tile_ptcl_ranges: Vec::new(),
            tile_ptcls: Vec::new(),
        }
    }

    pub fn render(&mut self, scene: &crate::scene::Scene) {
        <Self as Render>::render(self, scene);
    }

    pub fn render_profiled_flat(&mut self, scene: &crate::scene::Scene) -> RenderProfile {
        let total_start = std::time::Instant::now();
        let plan = scene.compile(0);
        let mut profile = RenderProfile::default();

        let start = std::time::Instant::now();
        self.scan(scene, ());
        profile.scan = start.elapsed();

        let start = std::time::Instant::now();
        self.cumsum(scene, ());
        profile.cumsum = start.elapsed();

        let mut image = Image::new(self.size.0, self.size.1, self.clear);
        let target_bounds = Bounds::canvas(scene.width, scene.height);

        for op in &plan.ops {
            let ExecOp::DrawBatch { draws, layer_stack } = op else {
                panic!("render_profiled_flat only supports scenes without layers");
            };

            let start = std::time::Instant::now();
            self.coarse(
                scene,
                (
                    &scene.draw_records,
                    draws.start..draws.end,
                    &plan.layer_stack_data,
                    layer_stack.clone(),
                ),
            );
            profile.coarse += start.elapsed();

            let start = std::time::Instant::now();
            self.fine(scene, (&mut image, target_bounds));
            profile.fine += start.elapsed();
        }

        self.image = image;
        profile.total = total_start.elapsed();
        profile
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    fn execute_plan(&mut self, scene: &crate::scene::Scene, plan: &ExecPlan, target: &mut Image) {
        self.execute_ops(
            scene,
            plan,
            &plan.ops,
            target,
            Bounds::canvas(scene.width, scene.height),
        );
    }

    fn execute_ops(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: &mut Image,
        root_bounds: Bounds,
    ) {
        for op in ops {
            match op {
                ExecOp::DrawBatch { draws, layer_stack } => self.execute_draw_batch(
                    scene,
                    plan,
                    draws.start,
                    draws.end,
                    layer_stack.clone(),
                    target,
                    root_bounds,
                ),
                ExecOp::BeginClip
                | ExecOp::EndClip
                | ExecOp::BeginOpacity
                | ExecOp::EndOpacity
                | ExecOp::BeginBlend
                | ExecOp::EndBlend => {}
                ExecOp::OffscreenLayer {
                    layer,
                    outer_stack,
                    children,
                } => self.execute_offscreen_layer(
                    scene,
                    plan,
                    OffscreenLayerRef {
                        layer,
                        outer_stack: outer_stack.clone(),
                        children,
                    },
                    target,
                    root_bounds,
                ),
            }
        }
    }

    fn execute_offscreen_layer(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        offscreen: OffscreenLayerRef<'_>,
        target: &mut Image,
        target_bounds: Bounds,
    ) {
        match offscreen.layer {
            Layer::ClipSdf { sdf, bounds } => {
                if bounds.intersect(target_bounds).is_empty() {
                    return;
                }
                let mut image = Image::new(
                    target_bounds.width(),
                    target_bounds.height(),
                    Color::TRANSPARENT,
                );
                self.execute_ops(scene, plan, offscreen.children, &mut image, target_bounds);

                let mut mask = rasterize_sdf_mask(sdf, *bounds, target_bounds);
                self.apply_outer_clip_stack_to_mask(
                    scene,
                    plan,
                    offscreen.outer_stack,
                    target_bounds,
                    &mut mask,
                );
                composite_src_over_masked_at(target, &image, &mask, target_bounds, target_bounds);
            }
            Layer::Filter {
                filter,
                sample_region,
            } => {
                let bounds = self.filter.filtered_region_bounds(
                    filter,
                    sample_region,
                    Bounds::canvas(scene.width, scene.height),
                );
                if bounds.is_empty() {
                    return;
                }
                let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
                self.execute_ops(scene, plan, offscreen.children, &mut image, bounds);
                self.filter.prepare(&mut image, filter, bounds).run();
                let mut mask = Image::new(bounds.width(), bounds.height(), Color::WHITE);
                self.apply_outer_clip_stack_to_mask(
                    scene,
                    plan,
                    offscreen.outer_stack,
                    bounds,
                    &mut mask,
                );
                composite_src_over_masked_at(target, &image, &mask, bounds, target_bounds);
            }
            Layer::Backdrop {
                filter,
                sample_region,
            } => {
                let bounds = self.filter.filtered_region_bounds(
                    filter,
                    sample_region,
                    Bounds::canvas(scene.width, scene.height),
                );
                if bounds.is_empty() {
                    return;
                }

                let mut backdrop = copy_image_region(target, bounds, target_bounds);
                self.filter.prepare(&mut backdrop, filter, bounds).run();

                let mut backdrop_mask = rasterize_region_mask(sample_region, bounds);
                self.apply_outer_clip_stack_to_mask(
                    scene,
                    plan,
                    offscreen.outer_stack.clone(),
                    bounds,
                    &mut backdrop_mask,
                );
                composite_src_over_masked_at(
                    target,
                    &backdrop,
                    &backdrop_mask,
                    bounds,
                    target_bounds,
                );

                let mut content = Image::new(
                    target_bounds.width(),
                    target_bounds.height(),
                    Color::TRANSPARENT,
                );
                self.execute_ops(scene, plan, offscreen.children, &mut content, target_bounds);
                let mut content_mask =
                    Image::new(target_bounds.width(), target_bounds.height(), Color::WHITE);
                self.apply_outer_clip_stack_to_mask(
                    scene,
                    plan,
                    offscreen.outer_stack,
                    target_bounds,
                    &mut content_mask,
                );
                composite_src_over_masked_at(
                    target,
                    &content,
                    &content_mask,
                    target_bounds,
                    target_bounds,
                );
            }
            _ => unreachable!(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_draw_batch(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        start: usize,
        end: usize,
        layer_stack: std::ops::Range<usize>,
        target: &mut Image,
        target_bounds: Bounds,
    ) {
        if start >= end {
            return;
        }
        self.coarse(
            scene,
            (
                &scene.draw_records,
                start..end,
                &plan.layer_stack_data,
                layer_stack,
            ),
        );
        self.fine(scene, (target, target_bounds));
    }

    fn apply_outer_clip_stack_to_mask(
        &self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        outer_stack: std::ops::Range<usize>,
        bounds: Bounds,
        mask: &mut Image,
    ) {
        for entry in &plan.layer_stack_data[outer_stack] {
            let LayerStackEntry::Clip { draw } = *entry else {
                continue;
            };
            let clip = self.rasterize_layer_mask(scene, draw as usize, bounds);
            for (dst, src) in mask.pixels.iter_mut().zip(clip.pixels) {
                let alpha = combine_alpha(((*dst >> 24) & 0xff) as u8, ((src >> 24) & 0xff) as u8);
                *dst = rgba8_pack([alpha, alpha, alpha, alpha]);
            }
        }
    }

    fn rasterize_layer_mask(
        &self,
        scene: &crate::scene::Scene,
        draw_ix: usize,
        bounds: Bounds,
    ) -> Image {
        let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
        let draw = &scene.draw_records[draw_ix];
        let Some(path_id) = draw.path_id else {
            return image;
        };
        let backdrop_record = &scene.bd_records[path_id as usize];
        let bbox = draw.tile_bbox(scene.width_in_tiles(), scene.height_in_tiles());
        let stride = backdrop_record.tile_x1 - backdrop_record.tile_x0;
        if stride == 0 {
            return image;
        }

        for tile_y in bbox.y0..bbox.y1 {
            for tile_x in bbox.x0..bbox.x1 {
                let local_x = tile_x - backdrop_record.tile_x0;
                let local_y = tile_y - backdrop_record.tile_y0;
                let local_ix = (local_y * stride + local_x) as usize;
                let backdrop_ix = backdrop_record.data_offset as usize + local_ix;
                let segment_range = self.tile_segment_ranges[backdrop_ix];
                let backdrop = self.backdrops[backdrop_ix];
                if segment_range.start == segment_range.end && backdrop == 0 {
                    continue;
                }

                let alpha = build_tile_alpha(
                    &self.segments[segment_range.start as usize..segment_range.end as usize],
                    backdrop,
                    draw.fill_rule,
                );
                let base_x = (tile_x * crate::TILE_SIZE) as i32;
                let base_y = (tile_y * crate::TILE_SIZE) as i32;
                let clip_x0 = base_x.max(bounds.x0);
                let clip_y0 = base_y.max(bounds.y0);
                let clip_x1 = (base_x + crate::TILE_SIZE as i32).min(bounds.x1);
                let clip_y1 = (base_y + crate::TILE_SIZE as i32).min(bounds.y1);
                if clip_x0 >= clip_x1 || clip_y0 >= clip_y1 {
                    continue;
                }

                for global_y in clip_y0..clip_y1 {
                    let row_start = ((global_y - base_y) as u32 * crate::TILE_SIZE) as usize;
                    for global_x in clip_x0..clip_x1 {
                        let tile_ix = row_start + (global_x - base_x) as usize;
                        let a = alpha[tile_ix];
                        if a == 0 {
                            continue;
                        }
                        let local_x = (global_x - bounds.x0) as u32;
                        let local_y = (global_y - bounds.y0) as u32;
                        let ix = (local_y * image.width + local_x) as usize;
                        image.pixels[ix] = rgba8_pack([a, a, a, a]);
                    }
                }
            }
        }
        image
    }
}

fn copy_image_region(source: &Image, bounds: Bounds, source_bounds: Bounds) -> Image {
    let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
    for y in 0..image.height {
        let src_y = bounds.y0 + y as i32 - source_bounds.y0;
        if src_y < 0 || src_y >= source.height as i32 {
            continue;
        }
        for x in 0..image.width {
            let src_x = bounds.x0 + x as i32 - source_bounds.x0;
            if src_x < 0 || src_x >= source.width as i32 {
                continue;
            }
            let src_ix = (src_y as u32 * source.width + src_x as u32) as usize;
            image.pixels[(y * image.width + x) as usize] = source.pixels[src_ix];
        }
    }
    image
}

fn rasterize_region_mask(region: &Region, bounds: Bounds) -> Image {
    match region {
        Region::Rect { rect, radius } => {
            let sdf = SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius: *radius,
            };
            rasterize_sdf_mask(&Sdf::Rect(sdf), rect_bounds(*rect), bounds)
        }
        Region::Path {
            path, transform, ..
        } => {
            let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
            let path = *transform * path;
            for y in 0..image.height {
                let py = bounds.y0 as f64 + y as f64 + 0.5;
                for x in 0..image.width {
                    let px = bounds.x0 as f64 + x as f64 + 0.5;
                    if path.contains(Point::new(px, py)) {
                        let ix = (y * image.width + x) as usize;
                        image.pixels[ix] = rgba8_pack([255, 255, 255, 255]);
                    }
                }
            }
            image
        }
    }
}

fn rasterize_sdf_mask(sdf: &Sdf, sdf_bounds: Bounds, bounds: Bounds) -> Image {
    let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
    let paint_bounds = sdf_bounds.intersect(bounds);
    if paint_bounds.is_empty() {
        return image;
    }

    let tile_x0 = paint_bounds.x0.div_euclid(crate::TILE_SIZE as i32);
    let tile_y0 = paint_bounds.y0.div_euclid(crate::TILE_SIZE as i32);
    let tile_x1 =
        (paint_bounds.x1 + crate::TILE_SIZE as i32 - 1).div_euclid(crate::TILE_SIZE as i32);
    let tile_y1 =
        (paint_bounds.y1 + crate::TILE_SIZE as i32 - 1).div_euclid(crate::TILE_SIZE as i32);

    for tile_y in tile_y0..tile_y1 {
        for tile_x in tile_x0..tile_x1 {
            let tile_bounds = Bounds::new(
                tile_x * crate::TILE_SIZE as i32,
                tile_y * crate::TILE_SIZE as i32,
                (tile_x + 1) * crate::TILE_SIZE as i32,
                (tile_y + 1) * crate::TILE_SIZE as i32,
            );
            let pixel_bounds = tile_bounds.intersect(paint_bounds);
            if pixel_bounds.is_empty() {
                continue;
            }

            if sdf.tile_is_solid(pixel_bounds) {
                write_sdf_mask_tile(&mut image, bounds, pixel_bounds, |_, _| 255);
                continue;
            }

            let mut area = [0.0; crate::BLOCK_SIZE as usize];
            sdf.fine_area(&mut area, tile_bounds, pixel_bounds);
            write_sdf_mask_tile(&mut image, bounds, pixel_bounds, |global_x, global_y| {
                let tile_ix = (global_y - tile_bounds.y0) as usize * crate::TILE_SIZE as usize
                    + (global_x - tile_bounds.x0) as usize;
                coverage_f32_to_u8(area[tile_ix])
            });
        }
    }

    image
}

fn write_sdf_mask_tile(
    image: &mut Image,
    bounds: Bounds,
    pixel_bounds: Bounds,
    mut alpha_at: impl FnMut(i32, i32) -> u8,
) {
    for global_y in pixel_bounds.y0..pixel_bounds.y1 {
        let local_y = (global_y - bounds.y0) as u32;
        for global_x in pixel_bounds.x0..pixel_bounds.x1 {
            let alpha = alpha_at(global_x, global_y);
            if alpha == 0 {
                continue;
            }
            let local_x = (global_x - bounds.x0) as u32;
            let ix = (local_y * image.width + local_x) as usize;
            image.pixels[ix] = rgba8_pack([alpha, alpha, alpha, alpha]);
        }
    }
}

fn rect_bounds(rect: peniko::kurbo::Rect) -> Bounds {
    Bounds::new(
        rect.x0.min(rect.x1).floor() as i32,
        rect.y0.min(rect.y1).floor() as i32,
        rect.x0.max(rect.x1).ceil() as i32,
        rect.y0.max(rect.y1).ceil() as i32,
    )
}

#[cfg(test)]
mod tests {
    use peniko::{
        Color, Compose, Mix,
        kurbo::{Affine, Circle, Rect, RoundedRect, Shape, Stroke},
    };

    use super::Renderer;
    use crate::{
        FillRule, Radius, Scene, StrokeWidths,
        shared::layer::{filter::Filter, region::Region},
    };

    fn render_single_rounded_rect() -> Renderer {
        let mut scene = Scene::new(320, 240);
        scene.push_path(
            RoundedRect::new(48.0, 48.0, 280.0, 200.0, (36.0, 36.0, 8.0, 8.0)).to_path(0.1),
            Color::from_rgb8(37, 99, 235),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.1,
        );

        let mut renderer = Renderer::new(320, 240, Color::WHITE);
        renderer.render(&scene);
        renderer
    }

    fn assert_rgb_close(actual: [u8; 4], expected: [u8; 4], tolerance: u8) {
        for channel in 0..4 {
            let delta = actual[channel].abs_diff(expected[channel]);
            assert!(
                delta <= tolerance,
                "channel {channel} expected {expected:?}, got {actual:?}"
            );
        }
    }

    #[test]
    fn rounded_rect_top_right_keeps_inside_filled() {
        let renderer = render_single_rounded_rect();

        assert_eq!(renderer.image().rgba8_at(260, 60), [37, 99, 235, 255]);
    }

    #[test]
    fn rounded_rect_top_right_keeps_outside_empty() {
        let renderer = render_single_rounded_rect();

        assert_eq!(renderer.image().rgba8_at(276, 52), [255, 255, 255, 255]);
    }

    #[test]
    fn plain_rect_right_edge_keeps_inside_filled() {
        let mut scene = Scene::new(320, 240);
        scene.push_path(
            Rect::new(48.0, 48.0, 280.0, 200.0).to_path(0.0),
            Color::from_rgb8(37, 99, 235),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let mut renderer = Renderer::new(320, 240, Color::WHITE);
        renderer.render(&scene);

        assert_eq!(renderer.image().rgba8_at(279, 60), [37, 99, 235, 255]);
        assert_eq!(renderer.image().rgba8_at(280, 60), [255, 255, 255, 255]);
    }

    #[test]
    fn clip_layer_masks_child_fill() {
        let mut scene = Scene::new(96, 96);
        scene.push_clip_layer(
            Rect::new(24.0, 24.0, 72.0, 72.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
        );
        scene.push_path(
            Rect::new(8.0, 8.0, 88.0, 88.0).to_path(0.0),
            Color::from_rgb8(37, 99, 235),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.pop_layer();

        let mut renderer = Renderer::new(96, 96, Color::WHITE);
        renderer.render(&scene);

        assert_eq!(renderer.image().rgba8_at(48, 48), [37, 99, 235, 255]);
        assert_eq!(renderer.image().rgba8_at(12, 48), [255, 255, 255, 255]);
    }

    #[test]
    fn sdf_rect_clip_masks_child_fill() {
        let mut scene = Scene::new(96, 96);
        scene.push_clip_sdf_rect_layer(Rect::new(24.0, 24.0, 72.0, 72.0), Radius::all(0.0));
        scene.push_rect(
            Rect::new(8.0, 8.0, 88.0, 88.0),
            Color::from_rgb8(37, 99, 235),
            FillRule::NonZero,
        );
        scene.pop_layer();

        let mut renderer = Renderer::new(96, 96, Color::WHITE);
        renderer.render(&scene);

        assert_eq!(renderer.image().rgba8_at(48, 48), [37, 99, 235, 255]);
        assert_eq!(renderer.image().rgba8_at(12, 48), [255, 255, 255, 255]);
    }

    #[test]
    fn sdf_rect_clip_keeps_subpixel_edge_coverage() {
        let mut scene = Scene::new(48, 48);
        scene.push_clip_sdf_rect_layer(Rect::new(16.25, 8.0, 32.25, 40.0), Radius::all(0.0));
        scene.push_rect(
            Rect::new(0.0, 0.0, 48.0, 48.0),
            Color::from_rgb8(255, 0, 0),
            FillRule::NonZero,
        );
        scene.pop_layer();

        let mut renderer = Renderer::new(48, 48, Color::WHITE);
        renderer.render(&scene);

        let edge = renderer.image().rgba8_at(16, 24);
        assert_eq!(renderer.image().rgba8_at(15, 24), [255, 255, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(17, 24), [255, 0, 0, 255]);
        assert_eq!(edge[0], 255);
        assert_eq!(edge[1], edge[2]);
        assert!(
            edge[1] > 0 && edge[1] < 255,
            "expected partially covered edge pixel, got {edge:?}"
        );
    }

    #[test]
    fn sdf_rounded_rect_clip_masks_corners() {
        let mut scene = Scene::new(96, 96);
        scene.push_clip_sdf_rect_layer(Rect::new(16.0, 16.0, 80.0, 80.0), Radius::all(16.0));
        scene.push_rect(
            Rect::new(0.0, 0.0, 96.0, 96.0),
            Color::from_rgb8(37, 99, 235),
            FillRule::NonZero,
        );
        scene.pop_layer();

        let mut renderer = Renderer::new(96, 96, Color::WHITE);
        renderer.render(&scene);

        assert_eq!(renderer.image().rgba8_at(48, 48), [37, 99, 235, 255]);
        assert_eq!(renderer.image().rgba8_at(17, 17), [255, 255, 255, 255]);
    }

    #[test]
    fn sdf_rect_stroke_renders_ring_without_filling_center() {
        let mut scene = Scene::new(64, 64);
        scene.push_rect_stroke(
            Rect::new(16.0, 16.0, 48.0, 48.0),
            Radius::all(0.0),
            Stroke::new(6.0),
            Color::from_rgb8(255, 0, 0),
            FillRule::NonZero,
        );

        let mut renderer = Renderer::new(64, 64, Color::WHITE);
        renderer.render(&scene);

        assert_eq!(renderer.image().rgba8_at(16, 32), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(32, 32), [255, 255, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(8, 32), [255, 255, 255, 255]);
    }

    #[test]
    fn sdf_rect_stroke_supports_per_side_widths() {
        let mut scene = Scene::new(64, 64);
        scene.push_rect_stroke_widths(
            Rect::new(20.0, 20.0, 44.0, 44.0),
            Radius::all(0.0),
            StrokeWidths {
                top: 2.0,
                right: 8.0,
                bottom: 4.0,
                left: 12.0,
            },
            Color::from_rgb8(255, 0, 0),
            FillRule::NonZero,
        );

        let mut renderer = Renderer::new(64, 64, Color::WHITE);
        renderer.render(&scene);

        assert_eq!(renderer.image().rgba8_at(16, 32), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(46, 32), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(32, 20), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(32, 44), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(32, 32), [255, 255, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(12, 32), [255, 255, 255, 255]);
    }

    #[test]
    fn sdf_circle_stroke_renders_ring_without_filling_center() {
        let mut scene = Scene::new(64, 64);
        scene.push_circle_stroke(
            Circle::new((32.0, 32.0), 14.0),
            Stroke::new(6.0),
            Color::from_rgb8(0, 128, 255),
            FillRule::NonZero,
        );

        let mut renderer = Renderer::new(64, 64, Color::WHITE);
        renderer.render(&scene);

        assert_eq!(renderer.image().rgba8_at(18, 32), [0, 128, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(32, 32), [255, 255, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(10, 32), [255, 255, 255, 255]);
    }

    #[test]
    fn nested_clip_layers_intersect_child_fill() {
        let mut scene = Scene::new(96, 96);
        scene.push_clip_layer(
            Rect::new(16.0, 16.0, 80.0, 80.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
        );
        scene.push_clip_layer(
            Rect::new(40.0, 8.0, 88.0, 88.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
        );
        scene.push_path(
            Rect::new(0.0, 0.0, 96.0, 96.0).to_path(0.0),
            Color::from_rgb8(37, 99, 235),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.pop_layer();
        scene.pop_layer();

        let mut renderer = Renderer::new(96, 96, Color::WHITE);
        renderer.render(&scene);

        assert_eq!(renderer.image().rgba8_at(48, 48), [37, 99, 235, 255]);
        assert_eq!(renderer.image().rgba8_at(24, 48), [255, 255, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(84, 48), [255, 255, 255, 255]);
    }

    #[test]
    fn outer_clip_does_not_clip_filter_source_before_blur() {
        let mut scene = Scene::new(96, 96);
        let clip = Circle::new((48.0, 48.0), 24.0).to_path(0.1);
        scene.push_clip_layer(clip.clone(), Affine::IDENTITY, 0.1);
        scene.push_filter_layer(
            Filter::Blur(8.0),
            Region::Path {
                path: clip,
                transform: Affine::IDENTITY,
                tolerance: 0.1,
            },
        );
        scene.push_path(
            Rect::new(0.0, 0.0, 96.0, 96.0).to_path(0.0),
            Color::from_rgb8(255, 0, 0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.pop_layer();
        scene.pop_layer();

        let mut renderer = Renderer::new(96, 96, Color::WHITE);
        renderer.render(&scene);

        assert_eq!(renderer.image().rgba8_at(70, 48), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(74, 48), [255, 255, 255, 255]);
    }

    #[test]
    fn filter_blur_outputs_expanded_bounds() {
        let mut scene = Scene::new(96, 96);
        let sample_rect = Rect::new(32.0, 32.0, 64.0, 64.0);
        scene.push_filter_layer(
            Filter::Blur(4.0),
            Region::rect(sample_rect, Radius::all(0.0)),
        );
        scene.push_path(
            sample_rect.to_path(0.0),
            Color::from_rgb8(255, 0, 0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.pop_layer();

        let mut renderer = Renderer::new(96, 96, Color::WHITE);
        renderer.render(&scene);

        let expanded_px = renderer.image().rgba8_at(28, 48);
        assert_eq!(expanded_px[0], 255);
        assert!(
            expanded_px[1] < 245 && expanded_px[2] < 245,
            "expected blur outside sample region, got {expanded_px:?}"
        );
    }

    #[test]
    fn backdrop_filter_samples_existing_target() {
        let mut scene = Scene::new(48, 24);
        scene.push_rect(
            Rect::new(0.0, 0.0, 48.0, 24.0),
            Color::from_rgb8(255, 0, 0),
            FillRule::NonZero,
        );
        scene.push_backdrop_layer(
            Filter::Invert(1.0),
            Region::rect(Rect::new(8.0, 4.0, 32.0, 20.0), Radius::all(0.0)),
        );
        scene.pop_layer();

        let mut renderer = Renderer::new(48, 24, Color::WHITE);
        renderer.render(&scene);

        assert_eq!(renderer.image().rgba8_at(12, 8), [0, 255, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(4, 8), [255, 0, 0, 255]);
    }

    #[test]
    fn opacity_layer_composites_children_as_isolated_group() {
        let mut scene = Scene::new(96, 96);
        scene.push_opacity_layer(
            Rect::new(8.0, 8.0, 88.0, 88.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
            0.5,
        );
        scene.push_path(
            Rect::new(16.0, 16.0, 64.0, 64.0).to_path(0.0),
            Color::from_rgb8(255, 0, 0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.push_path(
            Rect::new(32.0, 32.0, 80.0, 80.0).to_path(0.0),
            Color::from_rgb8(255, 0, 0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.pop_layer();

        let mut renderer = Renderer::new(96, 96, Color::WHITE);
        renderer.render(&scene);

        assert_rgb_close(renderer.image().rgba8_at(40, 40), [255, 128, 128, 255], 1);
    }

    #[test]
    fn blend_layer_composites_tile_group_through_layer_mask() {
        let mut scene = Scene::new(96, 96);
        scene.push_path(
            Rect::new(0.0, 0.0, 96.0, 96.0).to_path(0.0),
            Color::from_rgb8(128, 128, 128),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.push_blend_layer(
            Rect::new(16.0, 16.0, 80.0, 80.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
            Mix::Multiply,
            Compose::SrcOver,
        );
        scene.push_path(
            Rect::new(0.0, 0.0, 96.0, 96.0).to_path(0.0),
            Color::from_rgb8(255, 0, 0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        scene.pop_layer();

        let mut renderer = Renderer::new(96, 96, Color::WHITE);
        renderer.render(&scene);

        assert_rgb_close(renderer.image().rgba8_at(40, 40), [128, 0, 0, 255], 1);
        assert_eq!(renderer.image().rgba8_at(8, 40), [128, 128, 128, 255]);
    }
}
