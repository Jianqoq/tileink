use std::sync::atomic::AtomicU32;

use peniko::Color;

use crate::{
    cpu::pipelines::{cumsum::CumsumCpuPipeline, scan::ScanCpuPipeline},
    render::Render,
    shared::{
        execution::{ExecNode, ExecPlan},
        image::Image,
        layer::Layer,
        line_seg::LineSegment,
    },
};

pub struct Renderer {
    image: Image,
    clear: Color,
    scan: ScanCpuPipeline,
    cumsum: CumsumCpuPipeline,
    size: (u32, u32),

    backdrops: Vec<i32>,
    segments: Vec<LineSegment>,
    segments_bump: Vec<AtomicU32>,
}

impl Render for Renderer {
    type ScanArgs<'a> = ();

    type CumsumArgs<'a> = ();

    type CoarseArgs<'a> = ();

    type FineArgs<'a> = ();

    type ExecuteArgs<'a> = &'a mut Image;

    fn render(&mut self, scene: &crate::scene::Scene) {
        let mut iamge = Image::new(self.size.0, self.size.1, self.clear);
        self.execute(scene, &mut iamge);
    }

    fn execute(&mut self, scene: &crate::scene::Scene, args: Self::ExecuteArgs<'_>) {
        self.execute_plan(
            scene,
            &ExecPlan {
                nodes: scene.compile(0),
            },
            args,
        );
    }

    fn scan(&mut self, scene: &crate::scene::Scene, _: Self::ScanArgs<'_>) {
        let last_bd_record = scene.bd_records.last().unwrap().clone();
        self.backdrops.resize(
            last_bd_record.data_offset as usize + last_bd_record.data_len as usize,
            0,
        );
        self.segments.resize(
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
                &mut self.segments,
                &mut self.segments_bump,
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
        todo!()
    }

    fn fine(&mut self, scene: &crate::scene::Scene, args: Self::FineArgs<'_>) {
        todo!()
    }
}

impl Renderer {
    fn execute_plan(&mut self, scene: &crate::scene::Scene, plan: &ExecPlan, target: &mut Image) {
        for node in &plan.nodes {
            match node {
                ExecNode::DrawBatch { draws, state: _ } => {
                    self.execute_draw_batch(scene, draws.start, draws.end, target);
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
            Layer::Clip(_) | Layer::ClipSdf { .. } => todo!(),
            Layer::Opacity { opacity: _ } | Layer::Blend { blend: _ } => todo!(),
        }
    }

    fn execute_draw_batch(
        &mut self,
        scene: &crate::scene::Scene,
        start: usize,
        end: usize,
        target: &mut Image,
    ) {
        if start >= end {
            return;
        }
        let draw_records = &scene.draw_records[start..end];
        self.scan(scene, ());
        self.cumsum(scene, ());
    }
}
