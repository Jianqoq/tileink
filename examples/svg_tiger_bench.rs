#[path = "common/mod.rs"]
mod common;

use std::{
    error::Error,
    sync::mpsc,
    time::{Duration, Instant},
};

use peniko::Color;
use tileink::{WgpuRenderProfileReport, WgpuRenderer};

#[derive(Clone, Copy)]
struct Config {
    width: u32,
    warmup: usize,
    frames: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 900,
            warmup: 10,
            frames: 60,
        }
    }
}

#[derive(Clone, Copy)]
struct Stats {
    avg_ms: f64,
    min_ms: f64,
    max_ms: f64,
}

impl Stats {
    fn from_samples(samples: &[Duration]) -> Self {
        let mut min = f64::INFINITY;
        let mut max: f64 = 0.0;
        let mut sum = 0.0;
        for sample in samples {
            let ms = sample.as_secs_f64() * 1000.0;
            min = min.min(ms);
            max = max.max(ms);
            sum += ms;
        }
        Self {
            avg_ms: sum / samples.len() as f64,
            min_ms: min,
            max_ms: max,
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let input = common::example_asset("tiger.svg");
    let (scene, width, height) = common::load_svg_scene(input, config.width)?;
    let mut renderer = WgpuRenderer::new_default_device(width, height, Color::TRANSPARENT);
    let texture = output_texture(renderer.device(), width, height);

    println!(
        "svg tiger: {}x{}, warmup {}, frames {}",
        width, height, config.warmup, config.frames
    );
    println!("timing: CPU submit + GPU completion, no target readback\n");

    let stats = bench(&mut renderer, &scene, &texture, config)?;
    let profile = profile(&mut renderer, &scene, &texture, config)?;

    println!(
        "total {:>8.3} ms avg  [{:>8.3}, {:>8.3}]",
        stats.avg_ms, stats.min_ms, stats.max_ms
    );
    println!();
    println!("tileink stages ({} profiled frames)", profile.iterations());
    println!("{profile}");
    Ok(())
}

fn parse_config() -> Result<Config, Box<dyn Error>> {
    let mut config = Config::default();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut ix = 0;
    while ix < args.len() {
        let flag = args[ix].as_str();
        let Some(value) = args.get(ix + 1) else {
            return Err(format!("missing value for {flag}").into());
        };
        match flag {
            "--width" => config.width = value.parse()?,
            "--warmup" => config.warmup = value.parse()?,
            "--frames" => config.frames = value.parse()?,
            _ => return Err(format!("unknown argument {flag}").into()),
        }
        ix += 2;
    }
    if config.frames == 0 {
        return Err("--frames must be positive".into());
    }
    Ok(config)
}

fn bench(
    renderer: &mut WgpuRenderer,
    scene: &tileink::Canvas,
    texture: &wgpu::Texture,
    config: Config,
) -> Result<Stats, Box<dyn Error>> {
    for _ in 0..config.warmup {
        renderer.render_to_wgpu_texture(scene, texture)?;
        wait_for_gpu(renderer.device(), renderer.queue())?;
    }

    let mut samples = Vec::with_capacity(config.frames);
    for _ in 0..config.frames {
        let start = Instant::now();
        renderer.render_to_wgpu_texture(scene, texture)?;
        wait_for_gpu(renderer.device(), renderer.queue())?;
        samples.push(start.elapsed());
    }
    Ok(Stats::from_samples(&samples))
}

fn profile(
    renderer: &mut WgpuRenderer,
    scene: &tileink::Canvas,
    texture: &wgpu::Texture,
    config: Config,
) -> Result<WgpuRenderProfileReport, Box<dyn Error>> {
    for _ in 0..config.warmup {
        renderer.render_to_wgpu_texture(scene, texture)?;
        wait_for_gpu(renderer.device(), renderer.queue())?;
    }

    let mut report = WgpuRenderProfileReport::new();
    for _ in 0..config.frames {
        renderer.start_profile();
        renderer.render_to_wgpu_texture(scene, texture)?;
        wait_for_gpu(renderer.device(), renderer.queue())?;
        let profile = renderer.end_profile().clone();
        report.push(&profile);
    }
    Ok(report)
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

fn output_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tileink svg tiger bench output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
