#[path = "../examples/common/numeric_raster_cases.rs"]
mod raster_cases;

use criterion::{Criterion, criterion_group, criterion_main};
use peniko::Color;
use std::time::{Duration, Instant};
use tileink::WgpuRenderer;

#[path = "../examples/common/benchmark_gpu.rs"]
mod benchmark_gpu;

#[path = "../examples/common/mod.rs"]
mod common;

fn numeric_raster(c: &mut Criterion) {
    let api = std::env::var("TILEINK_BENCH_API").unwrap_or_else(|_| "vulkan".into());
    for portable in [false, true] {
        let (_, device, queue) =
            benchmark_gpu::device(&api, portable, false, wgpu::MemoryHints::Performance);
        let mode = if portable { "portable" } else { "native" };
        let mut group = c.benchmark_group(format!("{api}_{mode}_numeric_raster"));
        let mut renderer = WgpuRenderer::new(&device, &queue, 1, 1, Color::TRANSPARENT);
        for (name, fixture) in raster_cases::CASES {
            for width in raster_cases::WIDTHS {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("src/svg/tests")
                    .join(fixture);
                let (canvas, width, height) = common::load_svg_scene(path, width).unwrap();
                let target = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("numeric raster benchmark target"),
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
                renderer.render_to_wgpu_texture(&canvas, &target).unwrap();
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                // Sampling, not readback or compilation: transient external output forces
                // a complete render, and every measured frame waits for GPU completion.
                group.bench_function(format!("{name}-{width}"), |b| {
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
        }
        group.finish();
    }
}

// Global defaults keep Criterion CLI overrides effective for longer controlled runs.
// Group-level overrides otherwise silently discard --sample-size/--measurement-time.
criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(4));
    targets = numeric_raster
}
criterion_main!(benches);
