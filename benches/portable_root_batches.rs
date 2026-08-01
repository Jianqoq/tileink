use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Radius, WgpuRenderer};

const WIDTH: u32 = 1600;
const HEIGHT: u32 = 1000;
const BATCHES: u32 = 18;

fn portable_device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("portable benchmark requires a GPU adapter");
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tileink portable root-batch benchmark device"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .expect("portable benchmark requires an empty-feature device")
}

fn root_batches() -> Canvas {
    let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)),
        Radius::ZERO,
        Color::from_rgb8(12, 18, 28),
    );
    for index in 0..BATCHES {
        let inset = f64::from(index * 8);
        let rect = Rect::new(
            inset,
            inset,
            f64::from(WIDTH) - inset,
            f64::from(HEIGHT) - inset,
        );
        canvas.push_clip_sdf_rect_layer(rect, Radius::all(8.0));
        canvas.push_rect(
            rect,
            Radius::ZERO,
            if index % 2 == 0 {
                Color::from_rgba8(28, 108, 228, 18)
            } else {
                Color::from_rgba8(74, 198, 148, 18)
            },
        );
        canvas.pop_layer();
    }
    canvas
}

fn portable_root_batches(c: &mut Criterion) {
    let (device, queue) = portable_device();
    let mut renderer = WgpuRenderer::new(&device, &queue, WIDTH, HEIGHT, Color::TRANSPARENT);
    let canvas = root_batches();
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tileink portable root-batch benchmark target"),
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
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    renderer
        .render_to_wgpu_texture(&canvas, &target)
        .expect("portable benchmark warmup must render");
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("portable benchmark warmup must complete");

    let mut group = c.benchmark_group("portable_root_batches");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));
    group.bench_function(format!("{WIDTH}x{HEIGHT}/{BATCHES}"), |b| {
        b.iter_custom(|iterations| {
            let start = Instant::now();
            for _ in 0..iterations {
                renderer
                    .render_to_wgpu_texture(&canvas, &target)
                    .expect("portable root-batch frame must render");
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("portable root-batch frame must complete");
            }
            start.elapsed()
        });
    });
    group.finish();
}

criterion_group!(benches, portable_root_batches);
criterion_main!(benches);
