use std::{
    error::Error,
    hint::black_box,
    sync::mpsc,
    time::{Duration, Instant},
};

use peniko::Color;
use tileink::{Canvas, IncrementalRenderMode, WgpuRenderProfile, WgpuRenderer};

pub const WIDTH: u32 = 1024;
pub const HEIGHT: u32 = 1024;

#[derive(Clone, Copy)]
pub struct BenchConfig {
    pub warmup: usize,
    pub frames: usize,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            warmup: 3,
            frames: 20,
        }
    }
}

#[derive(Default)]
pub struct Measurements {
    pub wall: Vec<Duration>,
    pub cpu: Duration,
    pub collect: Duration,
    pub materialize: Duration,
    pub damage: Duration,
    pub prepare: Duration,
    pub dirty_tiles: u64,
    pub changed_tiles: u64,
    pub total_tiles: u32,
}

pub fn bench(
    seed: &WgpuRenderer,
    config: BenchConfig,
    frames: &[Canvas; 2],
    mode: IncrementalRenderMode,
) -> Result<Measurements, Box<dyn Error>> {
    let mut renderer = WgpuRenderer::new(
        seed.device(),
        seed.queue(),
        WIDTH,
        HEIGHT,
        Color::TRANSPARENT,
    );
    let mut renderer_config = renderer.incremental_render_config();
    renderer_config.mode = mode;
    renderer.set_incremental_render_config(renderer_config);
    let texture = output_texture(renderer.device());
    for index in 0..config.warmup {
        renderer.render_to_wgpu_texture(black_box(&frames[index % 2]), &texture)?;
        wait_for_gpu(renderer.device(), renderer.queue())?;
    }

    let mut measurements = Measurements::default();
    measurements.wall.reserve(config.frames);
    for index in 0..config.frames {
        let frame = &frames[(config.warmup + index) % frames.len()];
        renderer.start_profile();
        let started = Instant::now();
        renderer.render_to_wgpu_texture(black_box(frame), &texture)?;
        let profile = renderer.end_profile().clone();
        wait_for_gpu(renderer.device(), renderer.queue())?;
        measurements.wall.push(started.elapsed());
        measurements.cpu += profile.cpu_time();
        measurements.collect += stage(&profile, "retained.collect");
        measurements.materialize += stage(&profile, "retained.materialize");
        measurements.damage += stage(&profile, "retained.damage");
        measurements.prepare += stage(&profile, "prepare");
        let stats = renderer.incremental_render_stats();
        measurements.dirty_tiles += stats.dirty_tiles as u64;
        measurements.changed_tiles += stats.changed_tiles as u64;
        measurements.total_tiles = stats.total_tiles;
    }
    Ok(measurements)
}

fn stage(profile: &WgpuRenderProfile, name: &str) -> Duration {
    profile
        .entries()
        .iter()
        .filter(|entry| entry.name == name)
        .filter_map(|entry| entry.cpu_duration)
        .sum()
}

pub fn median_ms(samples: &[Duration]) -> f64 {
    let mut samples = samples.to_vec();
    samples.sort_unstable();
    ms(samples[samples.len() / 2])
}

pub fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn output_texture(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tileink retained benchmark output"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn wait_for_gpu(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), Box<dyn Error>> {
    let (tx, rx) = mpsc::channel();
    queue.on_submitted_work_done(move || {
        let _ = tx.send(());
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()?;
    Ok(())
}
