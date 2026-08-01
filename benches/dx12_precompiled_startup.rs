#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
#[cfg(target_os = "windows")]
use peniko::{Color, kurbo::Rect};
#[cfg(target_os = "windows")]
use tileink::{Canvas, Radius, WgpuRenderer};

#[cfg(target_os = "windows")]
fn dx12_precompiled_startup(c: &mut Criterion) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        flags: wgpu::InstanceFlags::empty(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        backend_options: wgpu::BackendOptions::default(),
        display: None,
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("DXIL startup benchmark requires a DX12 adapter");
    let required_features = wgpu::Features::PASSTHROUGH_SHADERS
        | wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
    assert!(
        adapter.features().contains(required_features),
        "DXIL startup benchmark requires passthrough shaders and binding arrays"
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tileink DXIL startup benchmark device"),
        required_features,
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .expect("DXIL startup benchmark requires a DX12 device");
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tileink DXIL startup benchmark target"),
        size: wgpu::Extent3d {
            width: 640,
            height: 480,
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
    let mut canvas = Canvas::new(640, 480, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 640.0, 480.0),
        Radius::ZERO,
        Color::from_rgb8(12, 18, 28),
    );
    for index in 0..24 {
        let x = f64::from((index % 6) * 96 + 12);
        let y = f64::from((index / 6) * 104 + 12);
        canvas.push_rect(
            Rect::new(x, y, x + 76.0, y + 84.0),
            Radius::all(6.0 + (index % 4) as f32),
            Color::from_rgba8(34, 116, 224, 180),
        );
    }

    let mut group = c.benchmark_group("dx12_precompiled_startup");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    group.bench_function("new_renderer_first_frame", |b| {
        b.iter_custom(|iterations| {
            let start = Instant::now();
            for _ in 0..iterations {
                let mut renderer = WgpuRenderer::new(&device, &queue, 640, 480, Color::TRANSPARENT);
                renderer
                    .render_to_wgpu_texture(&canvas, &target)
                    .expect("precompiled DXIL benchmark frame must render");
                assert!(
                    renderer.precompiled_dxil_pipeline_count() > 0,
                    "DX12 precompiled startup benchmark must exercise embedded DXIL"
                );
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("precompiled DXIL benchmark frame must complete");
            }
            start.elapsed()
        });
    });
    group.finish();
}

#[cfg(not(target_os = "windows"))]
fn dx12_precompiled_startup(_: &mut Criterion) {}

criterion_group!(benches, dx12_precompiled_startup);
criterion_main!(benches);
