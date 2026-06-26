use std::sync::atomic::AtomicU32;

use peniko::Color;

use crate::{
    TILE_SIZE,
    cpu::pipelines::{
        coarse::CoarseCpuPipeline, cumsum::CumsumCpuPipeline, fine::FineCpuPipeline,
        scan::ScanCpuPipeline,
    },
    render::Render,
    shared::{
        bounds::Bounds,
        draw_record::DrawRecord,
        execution::{ClipStackEntry, ExecOp, ExecPlan},
        image::{Image, rgba8_pack},
        layer::{Layer, blend::Blend, mask::MaskMode},
        line_seg::LineSegment,
        pixel::{pack_premul_rgba8, unpack_premul_rgba8},
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
    size: (u32, u32),

    backdrops: Vec<i32>,
    tile_segment_ranges: Vec<TileSegmentRange>,
    segments: Vec<LineSegment>,
    segments_bump: Vec<AtomicU32>,
    segment_tile_counts: Vec<AtomicU32>,
    segment_tile_cursors: Vec<AtomicU32>,
    packed_segments: Vec<LineSegment>,
    tile_ptcl_ranges: Vec<TilePtclRange>,
    tile_ptcls: Vec<TilePtcl>,
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
        self.segment_tile_counts.resize_with(
            last_bd_record.data_offset as usize + last_bd_record.data_len as usize,
            || AtomicU32::new(0),
        );
        self.segment_tile_cursors.resize_with(
            last_bd_record.data_offset as usize + last_bd_record.data_len as usize,
            || AtomicU32::new(0),
        );
        self.packed_segments.resize(
            last_bd_record.segment_start as usize + last_bd_record.segment_capacity as usize,
            LineSegment::default(),
        );
        self.segments_bump
            .resize_with(scene.bd_records.len(), || AtomicU32::new(0));
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
                &mut self.packed_segments,
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
        let (
            draw_records,
            draw_range,
            clip_stack_data,
            clip_stack_range,
        ) = args;
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
    fn execute_plan(&mut self, scene: &crate::scene::Scene, plan: &ExecPlan, target: &mut Image) {
        self.execute_ops(scene, plan, &plan.ops, target);
    }

    fn execute_ops(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: &mut Image,
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

        fn current_target_bounds(
            scene: &crate::scene::Scene,
            groups: &[GroupFrame],
        ) -> Bounds {
            match groups.last() {
                Some(GroupFrame::Opacity { bounds, .. })
                | Some(GroupFrame::Blend { bounds, .. }) => *bounds,
                None => Bounds::canvas(scene.width, scene.height),
            }
        }

        for op in ops {
            match op {
                ExecOp::DrawBatch {
                    draws,
                    clip_stack,
                } => {
                    let target_bounds = current_target_bounds(scene, &groups);
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
                    self.scale_image_opacity(&mut image, end_opacity);
                    let mask = self.rasterize_mask_image(scene, end_draw, end_bounds);
                    let dst = current_target(target, &mut groups);
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
                    let dst = current_target(target, &mut groups);
                    self.composite_blend_masked(dst, &image, &mask, end_mode, end_bounds);
                }
                ExecOp::OffscreenLayer { layer, children } => {
                    self.execute_offscreen_layer(
                        scene,
                        plan,
                        layer,
                        children,
                        current_target(target, &mut groups),
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
    ) {
        match layer {
            Layer::Filter { filter: _, region } => self.execute_ops(scene, plan, children, target),
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

    fn scale_image_opacity(&self, image: &mut Image, opacity: f32) {
        let opacity = opacity.clamp(0.0, 1.0);
        if opacity >= 1.0 {
            return;
        }
        for px in &mut image.pixels {
            let [r, g, b, a] = px.to_le_bytes();
            *px = rgba8_pack([
                ((r as f32 * opacity) + 0.5) as u8,
                ((g as f32 * opacity) + 0.5) as u8,
                ((b as f32 * opacity) + 0.5) as u8,
                ((a as f32 * opacity) + 0.5) as u8,
            ]);
        }
    }

    fn composite_blend_masked(
        &self,
        dst: &mut Image,
        content: &Image,
        mask: &Image,
        mode: peniko::BlendMode,
        bounds: Bounds,
    ) {
        let blend = Blend::new(mode.mix, mode.compose);
        for y in 0..content.height {
            let dst_y = bounds.y0 + y as i32;
            if dst_y < 0 || dst_y >= dst.height as i32 {
                continue;
            }
            for x in 0..content.width {
                let dst_x = bounds.x0 + x as i32;
                if dst_x < 0 || dst_x >= dst.width as i32 {
                    continue;
                }
                let ix = (y * content.width + x) as usize;
                let coverage = ((mask.pixels[ix] >> 24) & 0xff) as f32 / 255.0;
                if coverage <= 0.0 {
                    continue;
                }
                let mut src_px = unpack_premul_rgba8(content.pixels[ix]);
                for c in &mut src_px {
                    *c *= coverage;
                }
                let dst_ix = (dst_y as u32 * dst.width + dst_x as u32) as usize;
                let dst_px = unpack_premul_rgba8(dst.pixels[dst_ix]);
                dst.pixels[dst_ix] = pack_premul_rgba8(blend.blend(src_px, dst_px));
            }
        }
    }

    fn rasterize_mask_image(&self, scene: &crate::scene::Scene, draw_ix: usize, bounds: Bounds) -> Image {
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
