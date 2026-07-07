use std::{
    error::Error,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use tileink::{
    Brush, Canvas, FillRule, Image, ImageKey, PatternSampling, Radius, WgpuRenderProfileReport,
    WgpuRenderer,
};

#[derive(Clone, Copy)]
struct Config {
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
    image_size: u32,
    warmup: usize,
    frames: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            cols: 32,
            rows: 32,
            image_size: 64,
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

#[derive(Clone, Copy)]
enum CaseKind {
    Solid,
    ImageKey(PatternSampling),
    SceneImage(PatternSampling),
}

impl CaseKind {
    fn name(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::ImageKey(PatternSampling::Nearest) => "image-key-nearest",
            Self::ImageKey(PatternSampling::Bilinear) => "image-key-bilinear",
            Self::SceneImage(PatternSampling::Nearest) => "scene-image-nearest",
            Self::SceneImage(PatternSampling::Bilinear) => "scene-image-bilinear",
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let image = Arc::new(test_image(config.image_size, config.image_size));
    let image_key = ImageKey::new(1);
    let cases = [
        CaseKind::Solid,
        CaseKind::ImageKey(PatternSampling::Nearest),
        CaseKind::ImageKey(PatternSampling::Bilinear),
        CaseKind::SceneImage(PatternSampling::Nearest),
        CaseKind::SceneImage(PatternSampling::Bilinear),
    ];

    let mut renderer =
        WgpuRenderer::new_default_device(config.width, config.height, Color::TRANSPARENT);
    assert!(renderer.insert_image(image_key, Arc::clone(&image)));
    let texture = output_texture(renderer.device(), config.width, config.height);

    println!(
        "image key bench: {}x{}, grid {}x{} ({} draws), image {}x{}, warmup {}, frames {}",
        config.width,
        config.height,
        config.cols,
        config.rows,
        config.cols * config.rows,
        config.image_size,
        config.image_size,
        config.warmup,
        config.frames
    );
    println!("timing: CPU submit + GPU completion, no target readback\n");

    let mut baseline = None;
    for case in cases {
        let scene = build_scene(config, case, image_key, Arc::clone(&image));
        let stats = bench(&mut renderer, &scene, &texture, config)?;
        let profile = profile(&mut renderer, &scene, &texture, config)?;
        let native = renderer.last_frame_used_native_gpu();
        if matches!(case, CaseKind::Solid) {
            baseline = Some(stats.avg_ms);
        }

        let delta = baseline
            .map(|base| (stats.avg_ms / base - 1.0) * 100.0)
            .unwrap_or(0.0);
        println!(
            "{}: {:>8.3} ms avg [{:>8.3}, {:>8.3}], {:+.1}% vs solid, native_gpu={}",
            case.name(),
            stats.avg_ms,
            stats.min_ms,
            stats.max_ms,
            delta,
            native
        );
        println!("stages ({} profiled frames)", profile.iterations());
        println!("{profile}\n");
    }

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
            "--height" => config.height = value.parse()?,
            "--cols" => config.cols = value.parse()?,
            "--rows" => config.rows = value.parse()?,
            "--image-size" => config.image_size = value.parse()?,
            "--warmup" => config.warmup = value.parse()?,
            "--frames" => config.frames = value.parse()?,
            _ => return Err(format!("unknown argument {flag}").into()),
        }
        ix += 2;
    }
    if config.width == 0 || config.height == 0 {
        return Err("--width and --height must be positive".into());
    }
    if config.cols == 0 || config.rows == 0 {
        return Err("--cols and --rows must be positive".into());
    }
    if config.image_size == 0 {
        return Err("--image-size must be positive".into());
    }
    if config.frames == 0 {
        return Err("--frames must be positive".into());
    }
    Ok(config)
}

fn build_scene(config: Config, case: CaseKind, image_key: ImageKey, image: Arc<Image>) -> Canvas {
    let mut scene = Canvas::new(config.width, config.height, 1.0);
    let cell_w = config.width as f64 / config.cols as f64;
    let cell_h = config.height as f64 / config.rows as f64;

    for row in 0..config.rows {
        for col in 0..config.cols {
            let rect = Rect::new(
                col as f64 * cell_w,
                row as f64 * cell_h,
                (col + 1) as f64 * cell_w,
                (row + 1) as f64 * cell_h,
            );
            match case {
                CaseKind::Solid => {
                    let color = Color::from_rgba8(
                        (48 + (col * 5) % 160) as u8,
                        (80 + (row * 7) % 140) as u8,
                        (120 + ((row + col) * 3) % 100) as u8,
                        255,
                    );
                    scene.push_rect(rect, Radius::ZERO, color);
                }
                CaseKind::ImageKey(sampling) => {
                    let _ = scene.push_image_key(rect, image_key, sampling);
                }
                CaseKind::SceneImage(sampling) => {
                    let _ = scene.push_image(rect, Arc::clone(&image), sampling);
                }
            }
        }
    }

    // Add a small path after the image/solid fills so scan/coarse work is not a trivial single type.
    scene.push_path(
        Rect::new(
            config.width as f64 * 0.125,
            config.height as f64 * 0.125,
            config.width as f64 * 0.875,
            config.height as f64 * 0.875,
        )
        .to_path(0.1),
        Brush::Solid(Color::from_rgba8(255, 255, 255, 32)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene
}

fn test_image(width: u32, height: u32) -> Image {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let checker = ((x / 8) + (y / 8)) & 1;
            let r = ((x * 255) / width.max(1)) as u8;
            let g = ((y * 255) / height.max(1)) as u8;
            let b = if checker == 0 { 220 } else { 40 };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Image::from_rgba8(width, height, rgba)
}

fn bench(
    renderer: &mut WgpuRenderer,
    scene: &Canvas,
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
    scene: &Canvas,
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
        let _ = renderer.end_profile();
        wait_for_gpu(renderer.device(), renderer.queue())?;
        let profile = renderer.poll_profile().clone();
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
        label: Some("tileink image key bench output"),
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
