use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use peniko::Color;

use crate::{
    TILE_SIZE,
    cpu::{
        computes::blend::{
            composite_blend_masked_at, composite_src_over_masked_at, scale_image_opacity,
        },
        pipelines::{
            coarse::CoarseCpuPipeline, cumsum::CumsumCpuPipeline, filter::FilterCpuPipeline,
            fine::FineCpuPipeline, scan::ScanCpuPipeline,
        },
    },
    render::Render,
    shared::{
        bounds::Bounds,
        draw_record::DrawRecord,
        execution::{ClipStackEntry, ExecOp, ExecPlan},
        image::{Image, rgba8_pack},
        layer::Layer,
        line_seg::LineSegment,
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

impl Render for Renderer {
    type ScanArgs<'a> = ();

    type CumsumArgs<'a> = ();

    type CoarseArgs<'a> = (
        &'a [DrawRecord],
        std::ops::Range<usize>,
        &'a [ClipStackEntry],
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
        let last_bd_record = scene.bd_records.last().unwrap().clone();
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
                &scene.draw_records,
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
        let (draw_records, draw_range, clip_stack_data, clip_stack_range) = args;
        self.coarse
            .prepare(
                draw_records,
                draw_range,
                clip_stack_data,
                clip_stack_range,
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
            let ExecOp::DrawBatch { draws, clip_stack } = op else {
                panic!("render_profiled_flat only supports scenes without layers");
            };

            let start = std::time::Instant::now();
            self.coarse(
                scene,
                (
                    &scene.draw_records,
                    draws.start..draws.end,
                    &plan.clip_stack_data,
                    clip_stack.clone(),
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
        enum GroupFrame {
            Opacity {
                draw: usize,
                opacity: f32,
                bounds: Bounds,
                image: Image,
            },
            Blend {
                draw: usize,
                mode: peniko::BlendMode,
                bounds: Bounds,
                image: Image,
            },
        }

        let mut groups = Vec::<GroupFrame>::new();

        fn current_target<'a>(root: &'a mut Image, groups: &'a mut [GroupFrame]) -> &'a mut Image {
            match groups.last_mut() {
                Some(GroupFrame::Opacity { image, .. }) | Some(GroupFrame::Blend { image, .. }) => {
                    image
                }
                None => root,
            }
        }

        fn current_target_bounds(groups: &[GroupFrame], root_bounds: Bounds) -> Bounds {
            match groups.last() {
                Some(GroupFrame::Opacity { bounds, .. })
                | Some(GroupFrame::Blend { bounds, .. }) => *bounds,
                None => root_bounds,
            }
        }

        for op in ops {
            match op {
                ExecOp::DrawBatch { draws, clip_stack } => {
                    let target_bounds = current_target_bounds(&groups, root_bounds);
                    let dst = current_target(target, &mut groups);
                    self.execute_draw_batch(
                        scene,
                        plan,
                        draws.start,
                        draws.end,
                        clip_stack.clone(),
                        dst,
                        target_bounds,
                    );
                }
                ExecOp::BeginClip { draw: _, bounds: _ } | ExecOp::EndClip => {}
                ExecOp::BeginOpacity {
                    draw,
                    opacity,
                    bounds,
                } => {
                    groups.push(GroupFrame::Opacity {
                        draw: *draw,
                        opacity: *opacity,
                        bounds: *bounds,
                        image: Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT),
                    });
                }
                ExecOp::EndOpacity {
                    draw,
                    opacity,
                    bounds,
                } => {
                    let Some(GroupFrame::Opacity {
                        draw: end_draw,
                        opacity: end_opacity,
                        bounds: end_bounds,
                        mut image,
                    }) = groups.pop()
                    else {
                        continue;
                    };
                    debug_assert_eq!(*draw, end_draw);
                    debug_assert!((*opacity - end_opacity).abs() < f32::EPSILON);
                    debug_assert_eq!(*bounds, end_bounds);
                    scale_image_opacity(&mut image, end_opacity);
                    let mask = self.rasterize_mask_image(scene, end_draw, end_bounds);
                    let dst_bounds = current_target_bounds(&groups, root_bounds);
                    let dst = current_target(target, &mut groups);
                    composite_src_over_masked_at(dst, &image, &mask, end_bounds, dst_bounds);
                }
                ExecOp::BeginBlend { draw, mode, bounds } => {
                    groups.push(GroupFrame::Blend {
                        draw: *draw,
                        mode: *mode,
                        bounds: *bounds,
                        image: Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT),
                    });
                }
                ExecOp::EndBlend { draw, mode, bounds } => {
                    let Some(GroupFrame::Blend {
                        draw: end_draw,
                        mode: end_mode,
                        bounds: end_bounds,
                        image,
                    }) = groups.pop()
                    else {
                        continue;
                    };
                    debug_assert_eq!(*draw, end_draw);
                    debug_assert_eq!(*mode, end_mode);
                    debug_assert_eq!(*bounds, end_bounds);
                    let mask = self.rasterize_mask_image(scene, end_draw, end_bounds);
                    let dst_bounds = current_target_bounds(&groups, root_bounds);
                    let dst = current_target(target, &mut groups);
                    composite_blend_masked_at(dst, &image, &mask, end_mode, end_bounds, dst_bounds);
                }
                ExecOp::OffscreenLayer { layer, children } => {
                    let target_bounds = current_target_bounds(&groups, root_bounds);
                    self.execute_offscreen_layer(
                        scene,
                        plan,
                        layer,
                        children,
                        current_target(target, &mut groups),
                        target_bounds,
                    );
                }
            }
        }
    }

    fn execute_offscreen_layer(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        layer: &Layer,
        children: &[ExecOp],
        target: &mut Image,
        target_bounds: Bounds,
    ) {
        match layer {
            Layer::Filter { filter, region } => {
                let bounds = self.filter.filtered_region_bounds(
                    filter,
                    region,
                    Bounds::canvas(scene.width, scene.height),
                );
                if bounds.is_empty() {
                    return;
                }
                let mut image = Image::new(bounds.width(), bounds.height(), Color::TRANSPARENT);
                self.execute_ops(scene, plan, children, &mut image, bounds);
                self.filter.prepare(&mut image, filter, bounds).run();
                let mask = self.filter.rasterize_region_mask(region, bounds);
                composite_src_over_masked_at(target, &image, &mask, bounds, target_bounds);
            }
            Layer::Backdrop {
                filter: _,
                region: _,
            } => todo!(),
            _ => unreachable!(),
        }
    }

    fn execute_draw_batch(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        start: usize,
        end: usize,
        clip_stack: std::ops::Range<usize>,
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
                &plan.clip_stack_data,
                clip_stack,
            ),
        );
        self.fine(scene, (target, target_bounds));
    }

    fn rasterize_mask_image(
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
                let alpha = crate::cpu::computes::fine::build_tile_alpha(
                    &self.segments[segment_range.start as usize..segment_range.end as usize],
                    backdrop,
                    draw.fill_rule,
                );
                let base_x = tile_x * TILE_SIZE;
                let base_y = tile_y * TILE_SIZE;
                let clip_x0 = (base_x as i32).max(bounds.x0);
                let clip_y0 = (base_y as i32).max(bounds.y0);
                let clip_x1 = ((base_x + TILE_SIZE) as i32).min(bounds.x1);
                let clip_y1 = ((base_y + TILE_SIZE) as i32).min(bounds.y1);
                if clip_x0 >= clip_x1 || clip_y0 >= clip_y1 {
                    continue;
                }
                for global_y in clip_y0..clip_y1 {
                    let row_start = ((global_y as u32 - base_y) * TILE_SIZE) as usize;
                    for global_x in clip_x0..clip_x1 {
                        let a = alpha[row_start + (global_x as u32 - base_x) as usize];
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

#[cfg(test)]
mod tests {
    use peniko::{
        Color,
        kurbo::{Affine, Rect, RoundedRect, Shape},
    };

    use super::Renderer;
    use crate::{FillRule, Scene};

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
}
