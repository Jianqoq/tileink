use wgpu::{Device, Queue};

use crate::{
    memory::Allocation,
    render::Render,
    shared::{
        execution::DisplayItem,
        layer::{Layer, mask::MaskMode},
    },
    wgpu::{memory::Memory, pipelines::filter::FilterGpuPipeline},
};

pub struct Renderer {
    device: Device,
    queue: Queue,
    memory: Memory,
    filter: FilterGpuPipeline,
}

impl Render for Renderer {
    type ScanPrepared = ();
    type CumsumPrepared = ();
    type BinPrepared = ();
    type CoarsePrepared = ();
    type FinePrepared = ();
    type ExecuteArgs<'a> = (&'a mut wgpu::CommandEncoder, Allocation);

    fn render(&mut self, scene: &crate::scene::Scene) {
        self.filter.clear_dispatch_keepalive();
        let output =
            self.memory
                .allocate_image(scene.width, scene.height, peniko::Color::TRANSPARENT);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("classic_wgpu_render"),
            });
        self.execute(scene, (&mut encoder, output));
    }

    fn execute(&self, scene: &crate::scene::Scene, (encoder, target): Self::ExecuteArgs<'_>) {
        let mut batch_start = None;
        let mut batch_end = 0usize;

        for item in &scene.items {
            match item {
                DisplayItem::Draw(draw_ix) => {
                    if let Some(start) = batch_start {
                        if batch_end == *draw_ix {
                            batch_end = *draw_ix + 1;
                        } else {
                            self.execute_draw_batch(scene, start, batch_end);
                            batch_start = Some(*draw_ix);
                            batch_end = *draw_ix + 1;
                        }
                    } else {
                        batch_start = Some(*draw_ix);
                        batch_end = *draw_ix + 1;
                    }
                }
                DisplayItem::BeginLayer(layer) => {
                    if let Some(start) = batch_start.take() {
                        self.execute_draw_batch(scene, start, batch_end);
                    }
                    match layer {
                        Layer::Clip(_) | Layer::ClipSdf { .. } => {
                            // Keep clip layers in global coordinates for now. Nested path/SDF clips need the
                            // mask and content to agree on the full target coordinate space; making this local
                            // again requires translating every nested clip primitive consistently.
                            let content = self.memory.allocate_image(
                                scene.width,
                                scene.height,
                                peniko::Color::TRANSPARENT,
                            );
                            let mask = self.memory.allocate_image(
                                scene.width,
                                scene.height,
                                peniko::Color::TRANSPARENT,
                            );
                            self.execute(&layer.children, (encoder, content));
                            let mask_commands = clip_mask_commands(&layer.kind);
                            self.execute(&mask_commands, (encoder, mask));
                            self.filter.execute_mask_composite(
                                &mut self.memory,
                                encoder,
                                target,
                                content,
                                mask,
                                scene.width,
                                scene.height,
                                MaskMode::Alpha,
                            );
                        }
                        Layer::Opacity { opacity } => todo!(),
                        Layer::Blend { blend } => todo!(),
                        Layer::Filter { filter } => todo!(),
                        Layer::SvgFilter {
                            filters,
                            transform,
                            max_bounds,
                        } => todo!(),
                        Layer::BackdropFilter { filter, region } => todo!(),
                        Layer::Mask { mode } => todo!(),
                    }
                    // Layer enter/composite will be handled here once the GPU pipeline exists.
                }
                DisplayItem::EndLayer => {
                    if let Some(start) = batch_start.take() {
                        self.execute_draw_batch(scene, start, batch_end);
                    }
                    // Layer exit/composite will be handled here once the GPU pipeline exists.
                }
            }
        }

        if let Some(start) = batch_start {
            self.execute_draw_batch(scene, start, batch_end);
        }
    }

    fn prepare_scan(&self) {}

    fn flush(
        &self,
        _scan: Self::ScanPrepared,
        _cumsum: Self::CumsumPrepared,
        _bin: Self::BinPrepared,
        _coarse: Self::CoarsePrepared,
        _fine: Self::FinePrepared,
    ) {
        todo!()
    }

    fn prepare_cumsum(&self) {
        todo!()
    }

    fn prepare_bin(&self) {
        todo!()
    }

    fn prepare_coarse(&self) {
        todo!()
    }

    fn prepare_fine(&self) {
        todo!()
    }
}

impl Renderer {
    fn execute_draw_batch(&self, scene: &crate::scene::Scene, start: usize, end: usize) {
        if start >= end {
            return;
        }
        let _draw_records = &scene.draw_records[start..end];
        self.prepare_scan();
        self.prepare_cumsum();
        self.prepare_bin();
        self.prepare_coarse();
        self.prepare_fine();
        self.flush((), (), (), (), ());
    }
}
