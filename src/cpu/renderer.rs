use std::sync::atomic::AtomicU32;

use peniko::Color;

use crate::{
    cpu::pipelines::{
        coarse::CoarseCpuPipeline, cumsum::CumsumCpuPipeline, fine::FineCpuPipeline,
        scan::ScanCpuPipeline,
    },
    render::Render,
    shared::{
        draw_record::DrawRecord,
        execution::{ExecNode, ExecPlan, FusedLayerEntry},
        image::Image,
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
        &'a [FusedLayerEntry],
        std::ops::Range<usize>,
    );

    type FineArgs<'a> = &'a mut Image;

    type ExecuteArgs<'a> = &'a mut Image;

    fn render(&mut self, scene: &crate::scene::Scene) {
        let mut iamge = Image::new(self.size.0, self.size.1, self.clear);
        self.execute(scene, &mut iamge);
    }

    fn execute(&mut self, scene: &crate::scene::Scene, args: Self::ExecuteArgs<'_>) {
        let plan = scene.compile(0);
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
            fused_layers,
            fused_range,
        ) = args;
        self.coarse
            .prepare(
                draw_records,
                draw_range,
                fused_layers,
                fused_range,
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
        self.fine
            .prepare(
                &self.tile_ptcl_ranges,
                &self.tile_ptcls,
                &self.segments,
                target,
                (scene.width_in_tiles(), scene.height_in_tiles()),
            )
            .run();
    }
}

impl Renderer {
    fn execute_plan(&mut self, scene: &crate::scene::Scene, plan: &ExecPlan, target: &mut Image) {
        for node in &plan.nodes {
            match node {
                ExecNode::DrawBatch {
                    draws,
                    state: _,
                    fused_layers,
                } => {
                    self.execute_draw_batch(
                        scene,
                        plan,
                        draws.start,
                        draws.end,
                        fused_layers.clone(),
                        target,
                    );
                }
                ExecNode::OffscreenLayer { layer, children } => {
                    self.execute_offscreen_layer(scene, layer, children, target);
                }
            }
        }
    }

    fn execute_offscreen_layer(
        &mut self,
        _scene: &crate::scene::Scene,
        layer: &Layer,
        _children: &[ExecNode],
        target: &mut Image,
    ) {
        match layer {
            Layer::Filter { filter: _ } => todo!(),
            Layer::SvgFilter {
                filters: _,
                transform: _,
                max_bounds: _,
            } => todo!(),
            Layer::BackdropFilter {
                filter: _,
                region: _,
            } => todo!(),
            Layer::Mask { mode: _ } => todo!(),
            _ => unreachable!(),
        }
    }

    fn execute_draw_batch(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        start: usize,
        end: usize,
        fused_layers: std::ops::Range<usize>,
        target: &mut Image,
    ) {
        if start >= end {
            return;
        }
        self.scan(scene, ());
        self.cumsum(scene, ());
        self.coarse(
            scene,
            (
                &scene.draw_records,
                start..end,
                &plan.fused_layers,
                fused_layers,
            ),
        );
        self.fine(scene, target);
    }
}
