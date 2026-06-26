use wgpu::{BufferUsages, Device, Queue};

use crate::{
    render::Render,
    shared::{
        bd_record::BackdropRecord,
        execution::{BatchState, Command, CommandListId, ExecNode, ExecPlan, ROOT_COMMAND_LIST_ID},
        layer::Layer,
        line::Line,
        path::PathRecord,
    },
    wgpu::{
        buffer::{GpuImageBuffer, WgpuBuffer},
        pipelines::{
            cumsum::BackdropCumsumGpuPipeline, filter::FilterGpuPipeline, scan::ScanGpuPipeline,
        },
        types::tile_seg::TileSegment,
    },
};

pub struct Renderer {
    device: Device,
    queue: Queue,
    filter: FilterGpuPipeline,
    scan: ScanGpuPipeline,
    cumsum: BackdropCumsumGpuPipeline,

    bd_buffer: WgpuBuffer<BackdropRecord>,
    path_buffer: WgpuBuffer<PathRecord>,
    line_buffer: WgpuBuffer<Line>,
    segments_buffer: WgpuBuffer<TileSegment>,
    backdrop_pool_buffer: WgpuBuffer<i32>,
}

impl Renderer {
    pub fn new(device: Device, queue: Queue) -> Self {
        let filter = FilterGpuPipeline::new(&device);
        let scan = ScanGpuPipeline::new(&device);
        let cumsum = BackdropCumsumGpuPipeline::new(&device);
        let bd_buffer = WgpuBuffer::new(
            device.clone(),
            queue.clone(),
            BufferUsages::STORAGE,
            0,
            "bd_buffer",
        );
        let path_buffer = WgpuBuffer::new(
            device.clone(),
            queue.clone(),
            BufferUsages::STORAGE,
            0,
            "path_buffer",
        );
        let line_buffer = WgpuBuffer::new(
            device.clone(),
            queue.clone(),
            BufferUsages::STORAGE,
            0,
            "line_buffer",
        );
        let segments_buffer = WgpuBuffer::new(
            device.clone(),
            queue.clone(),
            BufferUsages::STORAGE,
            0,
            "segments_buffer",
        );
        let backdrop_pool_buffer = WgpuBuffer::new(
            device.clone(),
            queue.clone(),
            BufferUsages::STORAGE,
            0,
            "backdrop_pool_buffer",
        );
        Self {
            device,
            queue,
            filter,
            scan,
            cumsum,
            bd_buffer,
            path_buffer,
            line_buffer,
            segments_buffer,
            backdrop_pool_buffer,
        }
    }
}

impl Render for Renderer {
    type ScanArgs<'a> = &'a mut wgpu::CommandEncoder;
    type CumsumArgs<'a> = &'a mut wgpu::CommandEncoder;
    type CoarseArgs<'a> = &'a mut wgpu::CommandEncoder;
    type FineArgs<'a> = &'a mut wgpu::CommandEncoder;
    type ExecuteArgs<'a> = (&'a mut wgpu::CommandEncoder, &'a mut GpuImageBuffer);

    fn render(&mut self, scene: &crate::scene::Scene) {
        self.bd_buffer.clear();
        self.bd_buffer.extend_from_slice(&scene.bd_records);

        self.path_buffer.clear();
        self.path_buffer.extend_from_slice(&scene.path_records);

        self.line_buffer.clear();
        self.line_buffer.extend_from_slice(&scene.lines);

        self.segments_buffer.resize_zeroed(scene.tile_cnt as usize);
        self.backdrop_pool_buffer
            .resize_zeroed(scene.backdrop_pool_capacity as usize);
        self.filter.clear_dispatch_keepalive();
        let mut output = GpuImageBuffer::new(
            self.device.clone(),
            self.queue.clone(),
            wgpu::BufferUsages::STORAGE,
            scene.width,
            scene.height,
            "renderer_output_placeholder",
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("classic_wgpu_render"),
            });
        self.execute(scene, (&mut encoder, &mut output));
    }

    fn execute(&mut self, scene: &crate::scene::Scene, (encoder, target): Self::ExecuteArgs<'_>) {
        let plan = self.prepare_exec_plan(scene);
        self.execute_plan(scene, &plan, encoder, target);
    }

    fn scan(&mut self, scene: &crate::scene::Scene, encoder: Self::ScanArgs<'_>) {
        self.scan
            .prepare(
                &self.device,
                &self.path_buffer,
                &self.bd_buffer,
                &self.line_buffer,
                &self.segments_buffer,
                &self.backdrop_pool_buffer,
                scene.width_in_tiles(),
            )
            .run(encoder, &self.scan);
    }

    fn cumsum(&mut self, _scene: &crate::scene::Scene, encoder: Self::CumsumArgs<'_>) {
        self.cumsum
            .prepare(
                &self.device,
                &self.bd_buffer,
                &self.backdrop_pool_buffer,
                self.path_buffer.len() as u32,
            )
            .run(encoder, &self.cumsum);
    }

    fn coarse(&mut self, _scene: &crate::scene::Scene, _encoder: Self::CoarseArgs<'_>) {
        todo!()
    }

    fn fine(&mut self, _scene: &crate::scene::Scene, _encoder: Self::FineArgs<'_>) {
        todo!()
    }
}

impl Renderer {
    fn prepare_exec_plan(&self, scene: &crate::scene::Scene) -> ExecPlan {
        scene.compile(ROOT_COMMAND_LIST_ID)
    }

    fn execute_plan(
        &mut self,
        scene: &crate::scene::Scene,
        plan: &ExecPlan,
        encoder: &mut wgpu::CommandEncoder,
        target: &mut GpuImageBuffer,
    ) {
        for node in &plan.nodes {
            match node {
                ExecNode::DrawBatch {
                    draws,
                    state: _,
                    clip_stack: _,
                    opacity_stack: _,
                    blend_stack: _,
                } => {
                    self.execute_draw_batch(scene, draws.start, draws.end, encoder, target);
                }
                ExecNode::OffscreenLayer { layer, children } => {
                    self.execute_offscreen_layer(scene, layer, children, encoder, target);
                }
            }
        }
    }

    fn execute_offscreen_layer(
        &mut self,
        _scene: &crate::scene::Scene,
        layer: &Layer,
        _children: &[ExecNode],
        _encoder: &mut wgpu::CommandEncoder,
        _target: &mut GpuImageBuffer,
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
        encoder: &mut wgpu::CommandEncoder,
        target: &mut GpuImageBuffer,
    ) {
        if start >= end {
            return;
        }
        let draw_records = &scene.draw_records[start..end];
        self.scan(scene, encoder);
    }
}
