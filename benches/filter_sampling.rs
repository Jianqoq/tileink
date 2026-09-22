#[path = "support/filter_workload.rs"]
mod workload;
use peniko::Color;
use std::time::{Duration, Instant};
use workload::scene;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tileink::WgpuRenderer;

#[path = "../examples/common/benchmark_gpu.rs"]
mod benchmark_gpu;

fn target(device: &wgpu::Device, size: (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("filter sampling benchmark output"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn filter_sampling(c: &mut Criterion) {
    let api = std::env::var("TILEINK_BENCH_API").unwrap_or_else(|_| "vulkan".into());
    let portable = match std::env::var("TILEINK_WGPU_MODE").as_deref() {
        Ok("portable") | Err(std::env::VarError::NotPresent) => true,
        Ok("native") => false,
        _ => panic!("TILEINK_WGPU_MODE must be native or portable"),
    };
    let (_, device, queue) =
        benchmark_gpu::device(&api, portable, false, wgpu::MemoryHints::Performance);
    let mode = if portable { "portable" } else { "native" };
    let mut group = c.benchmark_group(format!("{api}_{mode}_filter_sampling"));
    for (name, sizes) in [
        ("fixed", &[(1280, 960)][..]),
        (
            "resize",
            &[(1280, 960), (1296, 976), (1288, 968), (1272, 952)][..],
        ),
    ] {
        let frames: Vec<_> = sizes
            .iter()
            .map(|&size| (scene(size), target(&device, size)))
            .collect();
        let mut renderer =
            WgpuRenderer::new(&device, &queue, sizes[0].0, sizes[0].1, Color::TRANSPARENT);
        // Warm the production filter pipelines and capacity growth outside timing.
        for _ in 0..2 {
            for (canvas, output) in &frames {
                renderer.render_to_wgpu_texture(canvas, output).unwrap();
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            }
        }
        let pipelines = renderer.pipeline_compilation_epoch();
        group.throughput(Throughput::Elements(frames.len() as u64));
        group.bench_function(name, |b| {
            b.iter_custom(|iterations| {
                let start = Instant::now();
                for _ in 0..iterations {
                    for (canvas, output) in &frames {
                        // Transient output forces each frame to execute the actual filter.
                        renderer.render_to_wgpu_texture(canvas, output).unwrap();
                        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    }
                }
                start.elapsed()
            })
        });
        assert_eq!(
            renderer.pipeline_compilation_epoch(),
            pipelines,
            "steady-state sampling compiled a pipeline"
        );
    }
    group.finish();
}

fn criterion_config() -> Criterion {
    let mut criterion = Criterion::default()
        .sample_size(60)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(8));
    if let Some(directory) = std::env::var_os("TILEINK_SAMPLING_BENCH_OUTPUT") {
        criterion = criterion.output_directory(std::path::Path::new(&directory));
    }
    criterion
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = filter_sampling
}
criterion_main!(benches);
