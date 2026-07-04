#[path = "common/mod.rs"]
mod common;

use std::{
    error::Error,
    fs,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

use peniko::{Color, kurbo::Affine};
use tileink::{Canvas, SvgOptions, WgpuRenderProfileReport, WgpuRenderer};
use usvg::{Node, Paint, PaintOrder, tiny_skia_path::PathSegment};
use vello::{
    AaConfig, AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions,
    Scene as VelloScene,
    kurbo::{
        Affine as VelloAffine, BezPath as VelloBezPath, Cap as VelloCap, Join as VelloJoin,
        Stroke as VelloStroke,
    },
    peniko::{Color as VelloColor, Fill as VelloFill},
};

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
            warmup: 30,
            frames: 120,
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
    let data = fs::read(&input)?;
    let usvg_options = usvg::Options {
        resources_dir: input.parent().map(Path::to_path_buf),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(&data, &usvg_options)?;
    let size = tree
        .size()
        .to_int_size()
        .scale_to_width(config.width)
        .ok_or("SVG size must be positive")?;
    let width = size.width();
    let height = size.height();
    let scale_x = width as f64 / tree.size().width() as f64;
    let scale_y = height as f64 / tree.size().height() as f64;

    let mut tileink_scene = Canvas::new(width, height);
    tileink_scene.push_svg_with_options(
        &tree,
        SvgOptions {
            transform: Affine::scale_non_uniform(scale_x, scale_y),
            ..SvgOptions::default()
        },
    )?;
    let (vello_scene, vello_path_count) = build_vello_scene(&tree, scale_x, scale_y)?;

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
        label: Some("tileink tiger vello compare device"),
        required_features,
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))?;

    let tileink_texture = output_texture(&device, width, height, "tileink tiger output");
    let vello_texture = output_texture(&device, width, height, "vello tiger output");
    let vello_view = vello_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut tileink = WgpuRenderer::new(&device, &queue, width, height, Color::TRANSPARENT);
    let mut vello = VelloRenderer::new(
        &device,
        RendererOptions {
            antialiasing_support: AaSupport::area_only(),
            ..RendererOptions::default()
        },
    )?;
    let vello_params = RenderParams {
        base_color: VelloColor::from_rgba8(0, 0, 0, 0),
        width,
        height,
        antialiasing_method: AaConfig::Area,
    };

    println!(
        "svg tiger compare: {}x{}, {} vello paths, adapter: {} ({:?})",
        width, height, vello_path_count, info.name, info.backend
    );
    println!(
        "timing: CPU submit + GPU completion, warmup {}, frames {}, no target readback\n",
        config.warmup, config.frames
    );

    let tileink_stats = bench_tileink(
        &mut tileink,
        &device,
        &queue,
        &tileink_scene,
        &tileink_texture,
        config,
    )?;
    let tileink_profile = profile_tileink(
        &mut tileink,
        &device,
        &queue,
        &tileink_scene,
        &tileink_texture,
        config,
    )?;
    let vello_stats = bench_vello(
        &mut vello,
        &device,
        &queue,
        &vello_scene,
        &vello_view,
        &vello_params,
        config,
    )?;

    print_result("tileink", tileink_stats);
    print_result("vello", vello_stats);
    println!(
        "ratio {:>8.2}x tileink/vello",
        tileink_stats.avg_ms / vello_stats.avg_ms
    );
    println!();
    println!(
        "tileink stages ({} profiled frames)",
        tileink_profile.iterations()
    );
    println!("{tileink_profile}");
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

fn build_vello_scene(
    tree: &usvg::Tree,
    scale_x: f64,
    scale_y: f64,
) -> Result<(VelloScene, usize), Box<dyn Error>> {
    let mut scene = VelloScene::new();
    let mut path_count = 0;
    push_vello_group(
        &mut scene,
        tree.root(),
        VelloAffine::scale_non_uniform(scale_x, scale_y),
        &mut path_count,
    )?;
    Ok((scene, path_count))
}

fn push_vello_group(
    scene: &mut VelloScene,
    group: &usvg::Group,
    transform: VelloAffine,
    path_count: &mut usize,
) -> Result<(), Box<dyn Error>> {
    if group.opacity().get() != 1.0
        || group.clip_path().is_some()
        || group.mask().is_some()
        || !group.filters().is_empty()
        || group.blend_mode() != usvg::BlendMode::Normal
    {
        return Err(format!("unsupported Vello compare SVG group: {}", group.id()).into());
    }

    for node in group.children() {
        match node {
            Node::Group(group) => push_vello_group(scene, group, transform, path_count)?,
            Node::Path(path) => {
                push_vello_path(scene, path, transform)?;
                *path_count += 1;
            }
            Node::Image(_) | Node::Text(_) => {
                return Err(format!("unsupported Vello compare SVG node: {}", node.id()).into());
            }
        }
    }
    Ok(())
}

fn push_vello_path(
    scene: &mut VelloScene,
    path: &usvg::Path,
    transform: VelloAffine,
) -> Result<(), Box<dyn Error>> {
    if !path.is_visible() {
        return Ok(());
    }
    let shape = vello_path(path.data());
    match path.paint_order() {
        PaintOrder::FillAndStroke => {
            push_vello_fill(scene, path.fill(), transform, &shape)?;
            push_vello_stroke(scene, path.stroke(), transform, &shape)?;
        }
        PaintOrder::StrokeAndFill => {
            push_vello_stroke(scene, path.stroke(), transform, &shape)?;
            push_vello_fill(scene, path.fill(), transform, &shape)?;
        }
    }
    Ok(())
}

fn push_vello_fill(
    scene: &mut VelloScene,
    fill: Option<&usvg::Fill>,
    transform: VelloAffine,
    shape: &VelloBezPath,
) -> Result<(), Box<dyn Error>> {
    let Some(fill) = fill else {
        return Ok(());
    };
    let style = match fill.rule() {
        usvg::FillRule::NonZero => VelloFill::NonZero,
        usvg::FillRule::EvenOdd => VelloFill::EvenOdd,
    };
    scene.fill(
        style,
        transform,
        vello_color(fill.paint(), fill.opacity().get())?,
        None,
        shape,
    );
    Ok(())
}

fn push_vello_stroke(
    scene: &mut VelloScene,
    stroke: Option<&usvg::Stroke>,
    transform: VelloAffine,
    shape: &VelloBezPath,
) -> Result<(), Box<dyn Error>> {
    let Some(stroke) = stroke else {
        return Ok(());
    };
    let style = VelloStroke {
        width: f64::from(stroke.width().get()),
        join: match stroke.linejoin() {
            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => VelloJoin::Miter,
            usvg::LineJoin::Round => VelloJoin::Round,
            usvg::LineJoin::Bevel => VelloJoin::Bevel,
        },
        miter_limit: f64::from(stroke.miterlimit().get()),
        start_cap: match stroke.linecap() {
            usvg::LineCap::Butt => VelloCap::Butt,
            usvg::LineCap::Round => VelloCap::Round,
            usvg::LineCap::Square => VelloCap::Square,
        },
        end_cap: match stroke.linecap() {
            usvg::LineCap::Butt => VelloCap::Butt,
            usvg::LineCap::Round => VelloCap::Round,
            usvg::LineCap::Square => VelloCap::Square,
        },
        dash_pattern: stroke
            .dasharray()
            .unwrap_or(&[])
            .iter()
            .map(|dash| f64::from(*dash))
            .collect(),
        dash_offset: f64::from(stroke.dashoffset()),
    };
    scene.stroke(
        &style,
        transform,
        vello_color(stroke.paint(), stroke.opacity().get())?,
        None,
        shape,
    );
    Ok(())
}

fn vello_color(paint: &Paint, opacity: f32) -> Result<VelloColor, Box<dyn Error>> {
    match paint {
        Paint::Color(color) => Ok(VelloColor::from_rgba8(
            color.red,
            color.green,
            color.blue,
            (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
        )),
        Paint::LinearGradient(_) | Paint::RadialGradient(_) | Paint::Pattern(_) => {
            Err("unsupported Vello compare paint server".into())
        }
    }
}

fn vello_path(path: &usvg::tiny_skia_path::Path) -> VelloBezPath {
    let mut out = VelloBezPath::new();
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => out.move_to((f64::from(p.x), f64::from(p.y))),
            PathSegment::LineTo(p) => out.line_to((f64::from(p.x), f64::from(p.y))),
            PathSegment::QuadTo(p0, p1) => out.quad_to(
                (f64::from(p0.x), f64::from(p0.y)),
                (f64::from(p1.x), f64::from(p1.y)),
            ),
            PathSegment::CubicTo(p0, p1, p2) => out.curve_to(
                (f64::from(p0.x), f64::from(p0.y)),
                (f64::from(p1.x), f64::from(p1.y)),
                (f64::from(p2.x), f64::from(p2.y)),
            ),
            PathSegment::Close => out.close_path(),
        }
    }
    out
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
        let _ = renderer.end_profile();
        // End the CPU profile before waiting; the wait only exists to make async GPU timestamps ready.
        wait_for_gpu(device, queue)?;
        let profile = renderer.poll_profile().clone();
        report.push(&profile);
    }
    Ok(report)
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

fn wait_for_gpu(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), Box<dyn Error>> {
    let (tx, rx) = mpsc::channel();
    queue.on_submitted_work_done(move || {
        let _ = tx.send(());
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()?;
    Ok(())
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

fn print_result(name: &str, stats: Stats) {
    println!(
        "{name:<7} {:>8.3} ms avg  [{:>8.3}, {:>8.3}]",
        stats.avg_ms, stats.min_ms, stats.max_ms
    );
}
