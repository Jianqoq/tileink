use std::{
    error::Error,
    hint::black_box,
    sync::mpsc,
    time::{Duration, Instant},
};

use peniko::Color;
use tileink::{Canvas, IncrementalRenderMode, RetainedScene, WgpuRenderProfile, WgpuRenderer};

pub const WIDTH: u32 = 1024;
pub const HEIGHT: u32 = 1024;

#[derive(Clone, Copy)]
pub struct BenchConfig {
    pub warmup: usize,
    pub frames: usize,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum MutationPhase {
    Insert,
    Remove,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            warmup: 3,
            frames: 20,
        }
    }
}

#[allow(dead_code)]
#[derive(Default)]
pub struct Measurements {
    pub wall: Vec<Duration>,
    pub transaction: Duration,
    pub cpu: Duration,
    pub collect: Duration,
    pub materialize: Duration,
    pub damage: Duration,
    pub prepare: Duration,
    pub scan: Duration,
    pub raster: Duration,
    pub plan_select: Duration,
    pub plan_execute: Duration,
    pub dirty_tiles: u64,
    pub changed_tiles: u64,
    pub draw_batches: u64,
    pub root_draw_batches: u64,
    pub total_tiles: u32,
    pub chunks_rebuilt: u64,
    pub plan_fragments_rebuilt: u64,
    pub full_scene_syncs: u64,
    pub cpu_copied_bytes: u64,
    pub gpu_uploaded_bytes: u64,
    pub tile_pages_rewritten: u64,
    pub tile_page_compactions: u64,
    pub arena_live_bytes: u64,
    pub arena_capacity_bytes: u64,
    pub arena_fragmentation: f64,
    pub arena_compactions: u64,
}

/// Measures the stateful retained API. `mutate` receives a monotonically increasing frame index
/// and is timed separately from rendering, so journal/transaction work cannot hide in setup.
#[allow(dead_code)]
pub fn bench_persistent(
    seed: &WgpuRenderer,
    config: BenchConfig,
    mut scene: RetainedScene,
    mode: IncrementalRenderMode,
    mut mutate: impl FnMut(&mut RetainedScene, usize),
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
    // Establish the renderer cursor before applying benchmark mutations. Otherwise the first
    // warmup mutation is folded into initial materialization, so alternating add/remove cases
    // accidentally measure one phase as full scene construction instead of incremental work.
    renderer.render_retained_to_wgpu_texture(black_box(&scene), &texture)?;
    wait_for_gpu(renderer.device(), renderer.queue())?;
    for index in 0..config.warmup {
        mutate(&mut scene, index);
        renderer.render_retained_to_wgpu_texture(black_box(&scene), &texture)?;
        wait_for_gpu(renderer.device(), renderer.queue())?;
    }

    let mut measurements = Measurements::default();
    measurements.wall.reserve(config.frames);
    for index in 0..config.frames {
        let frame = config.warmup + index;
        let transaction_started = Instant::now();
        mutate(&mut scene, frame);
        measurements.transaction += transaction_started.elapsed();
        renderer.start_profile();
        let started = Instant::now();
        renderer.render_retained_to_wgpu_texture(black_box(&scene), &texture)?;
        let profile = renderer.end_profile().clone();
        wait_for_gpu(renderer.device(), renderer.queue())?;
        measurements.wall.push(started.elapsed());
        accumulate(&mut measurements, &renderer, &profile);
    }
    Ok(measurements)
}

/// Measures one side of an alternating mutation independently while rendering the opposite side
/// only as untimed setup. Keeping these phase benchmarks permanent prevents a cheap insertion
/// from hiding a regressed removal (or vice versa) in their combined average.
#[allow(dead_code)]
pub fn bench_persistent_phase(
    seed: &WgpuRenderer,
    config: BenchConfig,
    mut scene: RetainedScene,
    mode: IncrementalRenderMode,
    phase: MutationPhase,
    mut mutate: impl FnMut(&mut RetainedScene, usize),
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
    renderer.render_retained_to_wgpu_texture(black_box(&scene), &texture)?;
    wait_for_gpu(renderer.device(), renderer.queue())?;

    let run = |renderer: &mut WgpuRenderer,
               scene: &mut RetainedScene,
               frame: usize,
               measured: bool,
               measurements: &mut Measurements,
               mutate: &mut dyn FnMut(&mut RetainedScene, usize)|
     -> Result<(), Box<dyn Error>> {
        let transaction_started = Instant::now();
        mutate(scene, frame);
        let transaction = transaction_started.elapsed();
        if measured {
            measurements.transaction += transaction;
            renderer.start_profile();
        }
        let started = Instant::now();
        renderer.render_retained_to_wgpu_texture(black_box(scene), &texture)?;
        let profile = measured.then(|| renderer.end_profile().clone());
        wait_for_gpu(renderer.device(), renderer.queue())?;
        if let Some(profile) = profile {
            measurements.wall.push(started.elapsed());
            accumulate(measurements, renderer, &profile);
        }
        Ok(())
    };

    let mut measurements = Measurements::default();
    measurements.wall.reserve(config.frames);
    for pair in 0..config.warmup + config.frames {
        let measured = pair >= config.warmup;
        let even = pair * 2;
        match phase {
            MutationPhase::Insert => {
                run(
                    &mut renderer,
                    &mut scene,
                    even,
                    measured,
                    &mut measurements,
                    &mut mutate,
                )?;
                run(
                    &mut renderer,
                    &mut scene,
                    even + 1,
                    false,
                    &mut measurements,
                    &mut mutate,
                )?;
            }
            MutationPhase::Remove => {
                run(
                    &mut renderer,
                    &mut scene,
                    even,
                    false,
                    &mut measurements,
                    &mut mutate,
                )?;
                run(
                    &mut renderer,
                    &mut scene,
                    even + 1,
                    measured,
                    &mut measurements,
                    &mut mutate,
                )?;
            }
        }
    }
    Ok(measurements)
}

#[allow(dead_code)]
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
        accumulate(&mut measurements, &renderer, &profile);
    }
    Ok(measurements)
}

fn accumulate(
    measurements: &mut Measurements,
    renderer: &WgpuRenderer,
    profile: &WgpuRenderProfile,
) {
    measurements.cpu += profile.cpu_time();
    measurements.collect += stage(profile, "retained.collect");
    measurements.materialize += stage(profile, "retained.materialize");
    measurements.damage +=
        stage(profile, "retained.damage") + stage(profile, "retained.damage.propagate");
    measurements.prepare += stage(profile, "prepare");
    measurements.scan += stage(profile, "scan") + stage(profile, "cumsum");
    measurements.raster += stage(profile, "coarse") + stage(profile, "fine");
    measurements.plan_select += stage(profile, "plan.active_batches");
    measurements.plan_execute += stage(profile, "plan.execute");
    let stats = renderer.incremental_render_stats();
    measurements.dirty_tiles += stats.dirty_tiles as u64;
    measurements.changed_tiles += stats.changed_tiles as u64;
    measurements.draw_batches += stats.draw_batches as u64;
    measurements.root_draw_batches += stats.root_draw_batches as u64;
    measurements.total_tiles = stats.total_tiles;
    measurements.chunks_rebuilt += stats.chunks_rebuilt as u64;
    measurements.plan_fragments_rebuilt += stats.plan_fragments_rebuilt as u64;
    measurements.full_scene_syncs += u64::from(stats.full_scene_sync);
    measurements.cpu_copied_bytes += stats.cpu_copied_bytes;
    measurements.gpu_uploaded_bytes += stats.gpu_uploaded_bytes;
    measurements.tile_pages_rewritten += stats.tile_pages_rewritten as u64;
    measurements.tile_page_compactions = stats.tile_page_compactions;
    measurements.arena_live_bytes += stats.arena_live_bytes;
    measurements.arena_capacity_bytes += stats.arena_capacity_bytes;
    measurements.arena_fragmentation += stats.arena_fragmentation as f64;
    measurements.arena_compactions = stats.arena_compactions;
}

fn stage(profile: &WgpuRenderProfile, name: &str) -> Duration {
    profile
        .entries()
        .iter()
        .filter(|entry| entry.name == name)
        .filter_map(|entry| entry.cpu_duration)
        .sum()
}

#[allow(dead_code)]
pub fn median_ms(samples: &[Duration]) -> f64 {
    let mut samples = samples.to_vec();
    samples.sort_unstable();
    ms(samples[samples.len() / 2])
}

#[allow(dead_code)]
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
