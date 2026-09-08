use criterion::{Criterion, criterion_group, criterion_main};
use peniko::Color;
use std::time::{Duration, Instant};
use tileink::WgpuRenderer;

#[path = "../examples/common/mod.rs"]
mod common;

fn pattern_sampling(c: &mut Criterion) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("pattern benchmark requires a hardware GPU");
    eprintln!("pattern sampling adapter: {:?}", adapter.get_info());
    let native = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        | wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
    assert!(
        adapter.features().contains(native),
        "benchmark must cover the native texture path without fallback"
    );
    for portable in [false, true] {
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("pattern sampling benchmark"),
            required_features: if portable {
                wgpu::Features::empty()
            } else {
                native
            },
            required_limits: adapter.limits(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .unwrap();
        let mode = if portable { "portable" } else { "native" };
        let mut group = c.benchmark_group(format!("{mode}_pattern_sampling"));
        group.sample_size(20);
        group.warm_up_time(Duration::from_secs(2));
        group.measurement_time(Duration::from_secs(4));
        for width in [300, 1600] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/svg/tests/painting/context/with-pattern-and-transform-in-use.svg");
            let (canvas, width, height) = common::load_svg_scene(path, width).unwrap();
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("pattern benchmark target"),
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
            });
            let mut renderer =
                WgpuRenderer::new(&device, &queue, width, height, Color::TRANSPARENT);
            renderer.render_to_wgpu_texture(&canvas, &target).unwrap();
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            // Sampling, not readback or compilation: transient external output forces
            // a complete render, and every measured frame waits for GPU completion.
            group.bench_function(format!("rotated-context-{width}"), |b| {
                b.iter_custom(|iterations| {
                    let start = Instant::now();
                    for _ in 0..iterations {
                        renderer.render_to_wgpu_texture(&canvas, &target).unwrap();
                        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    }
                    start.elapsed()
                });
            });
        }
        group.finish();
    }
}

criterion_group!(benches, pattern_sampling);
criterion_main!(benches);
