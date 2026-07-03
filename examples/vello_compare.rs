use std::{
    env,
    error::Error,
    sync::mpsc,
    time::{Duration, Instant},
};

use peniko::{Color, kurbo::Rect};
use tileink::{CandleStick, Canvas, Radius, WgpuRenderProfileReport, WgpuRenderer};
use vello::{
    AaConfig, AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions,
    Scene as VelloScene,
    kurbo::{Affine as VelloAffine, Rect as VelloRect},
    peniko::{Color as VelloColor, Fill as VelloFill},
};

#[derive(Clone, Copy)]
struct Config {
    width: u32,
    height: u32,
    warmup: usize,
    frames: usize,
    candles: usize,
    rects: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            warmup: 5,
            frames: 30,
            candles: 10_000,
            rects: 20_000,
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
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    let info = adapter.get_info();
    let required_features = adapter.features()
        & (wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TIMESTAMP_QUERY);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tileink vello compare device"),
        required_features,
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))?;

    println!(
        "adapter: {} ({:?}), {}x{}, warmup {}, frames {}",
        info.name, info.backend, config.width, config.height, config.warmup, config.frames
    );
    println!("timing: CPU submit + GPU completion, no readback\n");

    let tileink_texture = output_texture(
        &device,
        config.width,
        config.height,
        "tileink compare output",
    );
    let vello_texture =
        output_texture(&device, config.width, config.height, "vello compare output");
    let vello_view = vello_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let mut tileink = WgpuRenderer::new(&device, &queue, config.width, config.height, Color::WHITE);
    let mut vello = VelloRenderer::new(
        &device,
        RendererOptions {
            antialiasing_support: AaSupport::area_only(),
            ..RendererOptions::default()
        },
    )?;
    let vello_params = RenderParams {
        base_color: VelloColor::from_rgb8(255, 255, 255),
        width: config.width,
        height: config.height,
        antialiasing_method: AaConfig::Area,
    };

    let (tileink_candles, vello_candles) = build_candlestick_scenes(config);
    let candles_tileink = bench_tileink(
        &mut tileink,
        &device,
        &queue,
        &tileink_candles,
        &tileink_texture,
        config,
    )?;
    let candles_tileink_profile = profile_tileink(
        &mut tileink,
        &device,
        &queue,
        &tileink_candles,
        &tileink_texture,
        config,
    )?;
    let candles_vello = bench_vello(
        &mut vello,
        &device,
        &queue,
        &vello_candles,
        &vello_view,
        &vello_params,
        config,
    )?;

    let (tileink_rects, vello_rects) = build_rect_scenes(config);
    let rects_tileink = bench_tileink(
        &mut tileink,
        &device,
        &queue,
        &tileink_rects,
        &tileink_texture,
        config,
    )?;
    let rects_tileink_profile = profile_tileink(
        &mut tileink,
        &device,
        &queue,
        &tileink_rects,
        &tileink_texture,
        config,
    )?;
    let rects_vello = bench_vello(
        &mut vello,
        &device,
        &queue,
        &vello_rects,
        &vello_view,
        &vello_params,
        config,
    )?;

    print_result(
        "candlestick",
        config.candles,
        candles_tileink,
        candles_vello,
    );
    print_profile("tileink candlestick stages", &candles_tileink_profile);
    print_result("solid rects", config.rects, rects_tileink, rects_vello);
    print_profile("tileink solid rect stages", &rects_tileink_profile);
    Ok(())
}

fn parse_config() -> Result<Config, Box<dyn Error>> {
    let mut config = Config::default();
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let Some(value) = args.get(i + 1) else {
            return Err(format!("missing value for {flag}").into());
        };
        match flag {
            "--width" => config.width = value.parse()?,
            "--height" => config.height = value.parse()?,
            "--warmup" => config.warmup = value.parse()?,
            "--frames" => config.frames = value.parse()?,
            "--candles" => config.candles = value.parse()?,
            "--rects" => config.rects = value.parse()?,
            _ => return Err(format!("unknown argument {flag}").into()),
        }
        i += 2;
    }
    if config.frames == 0 {
        return Err("--frames must be positive".into());
    }
    Ok(config)
}

fn output_texture(device: &wgpu::Device, width: u32, height: u32, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
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

fn bench_tileink(
    renderer: &mut WgpuRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Canvas,
    texture: &wgpu::Texture,
    config: Config,
) -> Result<Stats, Box<dyn Error>> {
    for _ in 0..config.warmup {
        renderer.render_to_wgpu_texture(scene, texture)?;
        wait_for_gpu(device, queue)?;
    }
    let mut samples = Vec::with_capacity(config.frames);
    for _ in 0..config.frames {
        let start = Instant::now();
        renderer.render_to_wgpu_texture(scene, texture)?;
        wait_for_gpu(device, queue)?;
        samples.push(start.elapsed());
    }
    Ok(Stats::from_samples(&samples))
}

fn bench_vello(
    renderer: &mut VelloRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &VelloScene,
    texture: &wgpu::TextureView,
    params: &RenderParams,
    config: Config,
) -> Result<Stats, Box<dyn Error>> {
    for _ in 0..config.warmup {
        renderer.render_to_texture(device, queue, scene, texture, params)?;
        wait_for_gpu(device, queue)?;
    }
    let mut samples = Vec::with_capacity(config.frames);
    for _ in 0..config.frames {
        let start = Instant::now();
        renderer.render_to_texture(device, queue, scene, texture, params)?;
        wait_for_gpu(device, queue)?;
        samples.push(start.elapsed());
    }
    Ok(Stats::from_samples(&samples))
}

fn profile_tileink(
    renderer: &mut WgpuRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Canvas,
    texture: &wgpu::Texture,
    config: Config,
) -> Result<WgpuRenderProfileReport, Box<dyn Error>> {
    for _ in 0..config.warmup {
        renderer.render_to_wgpu_texture(scene, texture)?;
        wait_for_gpu(device, queue)?;
    }

    let mut report = WgpuRenderProfileReport::new();
    for _ in 0..config.frames {
        renderer.start_profile();
        renderer.render_to_wgpu_texture(scene, texture)?;
        wait_for_gpu(device, queue)?;
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

fn build_candlestick_scenes(config: Config) -> (Canvas, VelloScene) {
    let mut tileink_scene = Canvas::new(config.width, config.height);
    let mut vello_scene = VelloScene::new();
    tileink_scene.push_rect(
        Rect::new(0.0, 0.0, f64::from(config.width), f64::from(config.height)),
        Radius::ZERO,
        Color::WHITE,
    );
    vello_scene.fill(
        VelloFill::NonZero,
        VelloAffine::IDENTITY,
        VelloColor::from_rgb8(255, 255, 255),
        None,
        &VelloRect::new(0.0, 0.0, f64::from(config.width), f64::from(config.height)),
    );

    let stride = 6.0;
    let columns = (config.width as f32 / stride).max(1.0) as usize;
    for i in 0..config.candles {
        let column = i % columns;
        let row = i / columns;
        let x = column as f32 * stride + stride * 0.5;
        let base = 36.0 + (row as f32 * 28.0) % (config.height as f32 - 72.0).max(1.0);
        let high = (base - 18.0 - (i % 17) as f32 * 0.7).max(3.0);
        let low = (base + 18.0 + (i % 13) as f32 * 0.9).min(config.height as f32 - 3.0);
        let open = high + (low - high) * (0.25 + (i % 11) as f32 * 0.035);
        let close = high + (low - high) * (0.75 - (i % 7) as f32 * 0.045);
        let up = close < open;
        let color = if up {
            Color::from_rgb8(27, 150, 96)
        } else {
            Color::from_rgb8(214, 75, 68)
        };
        let vello_color = if up {
            VelloColor::from_rgb8(27, 150, 96)
        } else {
            VelloColor::from_rgb8(214, 75, 68)
        };
        let body_width = if i % 3 == 0 { 4 } else { 5 };
        let wick_width = if i % 5 == 0 { 2 } else { 1 };

        tileink_scene.push_candlestick(
            CandleStick::new(x, high, low, open, close, body_width, wick_width),
            color,
        );

        let wick_half = wick_width as f64 * 0.5;
        vello_scene.fill(
            VelloFill::NonZero,
            VelloAffine::IDENTITY,
            vello_color,
            None,
            &VelloRect::new(
                f64::from(x) - wick_half,
                f64::from(high.min(low)),
                f64::from(x) + wick_half,
                f64::from(high.max(low)),
            ),
        );

        let body_half = body_width as f64 * 0.5;
        let mut y0 = open.min(close);
        let mut y1 = open.max(close);
        if y0 == y1 {
            y0 -= 0.5;
            y1 += 0.5;
        }
        vello_scene.fill(
            VelloFill::NonZero,
            VelloAffine::IDENTITY,
            vello_color,
            None,
            &VelloRect::new(
                f64::from(x) - body_half,
                f64::from(y0),
                f64::from(x) + body_half,
                f64::from(y1),
            ),
        );
    }
    (tileink_scene, vello_scene)
}

fn build_rect_scenes(config: Config) -> (Canvas, VelloScene) {
    let mut tileink_scene = Canvas::new(config.width, config.height);
    let mut vello_scene = VelloScene::new();
    tileink_scene.push_rect(
        Rect::new(0.0, 0.0, f64::from(config.width), f64::from(config.height)),
        Radius::ZERO,
        Color::WHITE,
    );
    vello_scene.fill(
        VelloFill::NonZero,
        VelloAffine::IDENTITY,
        VelloColor::from_rgb8(255, 255, 255),
        None,
        &VelloRect::new(0.0, 0.0, f64::from(config.width), f64::from(config.height)),
    );

    let stride = 10.0;
    let columns = (config.width as f64 / stride).max(1.0) as usize;
    for i in 0..config.rects {
        let column = i % columns;
        let row = i / columns;
        let x = column as f64 * stride + 1.0;
        let y = (row as f64 * stride + 1.0) % (f64::from(config.height) - 9.0).max(1.0);
        let rect = Rect::new(x, y, x + 7.0 + (i % 3) as f64, y + 7.0);
        let r = (37 * i % 210 + 30) as u8;
        let g = (67 * i % 210 + 30) as u8;
        let b = (97 * i % 210 + 30) as u8;
        let color = Color::from_rgb8(r, g, b);
        tileink_scene.push_rect(rect, Radius::ZERO, color);
        vello_scene.fill(
            VelloFill::NonZero,
            VelloAffine::IDENTITY,
            VelloColor::from_rgb8(r, g, b),
            None,
            &VelloRect::new(rect.x0, rect.y0, rect.x1, rect.y1),
        );
    }
    (tileink_scene, vello_scene)
}

fn print_result(name: &str, count: usize, tileink: Stats, vello: Stats) {
    println!("{name} ({count} primitives)");
    println!(
        "  tileink {:>8.3} ms avg  [{:>8.3}, {:>8.3}]",
        tileink.avg_ms, tileink.min_ms, tileink.max_ms
    );
    println!(
        "  vello   {:>8.3} ms avg  [{:>8.3}, {:>8.3}]",
        vello.avg_ms, vello.min_ms, vello.max_ms
    );
    println!(
        "  ratio   {:>8.2}x vello/tileink\n",
        vello.avg_ms / tileink.avg_ms
    );
}

fn print_profile(name: &str, profile: &WgpuRenderProfileReport) {
    println!("{name} ({} profiled frames)", profile.iterations());
    println!("{profile}\n");
}
