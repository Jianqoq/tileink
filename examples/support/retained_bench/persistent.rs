use super::{BenchContext, Measurements, output_texture, wait_for_gpu};
use std::{error::Error, hint::black_box, time::Instant};
use tileink::{IncrementalRenderConfig, RetainedScene, WgpuRenderer};

/// A warmed retained scene whose renderer and mutation cursor survive Criterion
/// samples. Rebuilding per sample repeats unmeasured setup and loses real history.
pub struct PersistentSession {
    renderer: WgpuRenderer,
    texture: wgpu::Texture,
    scene: RetainedScene,
    frame: usize,
    profile: bool,
}

impl PersistentSession {
    pub fn new(
        context: &BenchContext,
        scene: RetainedScene,
        renderer_config: IncrementalRenderConfig,
        profile: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let mut renderer = context.renderer();
        renderer.set_incremental_render_config(renderer_config);
        let texture = output_texture(renderer.device());
        // Establish the cursor before mutations, so initial construction cannot
        // absorb one half of an alternating insert/remove workload.
        renderer.render_retained_to_wgpu_texture(black_box(&scene), &texture)?;
        wait_for_gpu(renderer.device(), renderer.queue())?;
        Ok(Self {
            renderer,
            texture,
            scene,
            frame: 0,
            profile,
        })
    }

    pub fn warm(
        &mut self,
        frames: usize,
        mut mutate: impl FnMut(&mut RetainedScene, usize),
    ) -> Result<(), Box<dyn Error>> {
        for _ in 0..frames {
            mutate(&mut self.scene, self.frame);
            self.renderer
                .render_retained_to_wgpu_texture(black_box(&self.scene), &self.texture)?;
            wait_for_gpu(self.renderer.device(), self.renderer.queue())?;
            self.frame += 1;
        }
        Ok(())
    }

    pub fn measure(
        &mut self,
        frames: usize,
        mut mutate: impl FnMut(&mut RetainedScene, usize),
    ) -> Result<Measurements, Box<dyn Error>> {
        let mut measurements = Measurements::default();
        measurements.wall.reserve(frames);
        for _ in 0..frames {
            let started = Instant::now();
            mutate(&mut self.scene, self.frame);
            measurements.transaction += started.elapsed();
            let scene = &self.scene;
            let texture = &self.texture;
            measurements.record_frame(&mut self.renderer, started, self.profile, |renderer| {
                renderer.render_retained_to_wgpu_texture(black_box(scene), texture)
            })?;
            self.frame += 1;
        }
        Ok(measurements)
    }
}
