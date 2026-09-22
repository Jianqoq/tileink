#[path = "support/root_workload.rs"]
mod workload;
use workload::benchmark_scenes;
#[path = "../examples/common/benchmark_gpu.rs"]
mod benchmark_gpu;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use peniko::Color;
use tileink::Renderer;

fn benchmark_device(portable: bool) -> (wgpu::Device, wgpu::Queue) {
    let api = std::env::var("TILEINK_BENCH_API").unwrap_or_else(|_| "vulkan".into());
    let (_, device, queue) =
        benchmark_gpu::device(&api, portable, false, wgpu::MemoryHints::Performance);
    (device, queue)
}

fn root_batch_submission(c: &mut Criterion) {
    // Keep both texture paths and small workloads in the matrix: an extra submit only helps
    // when enough GPU work can overlap the remaining CPU command-buffer completion.
    for portable in [false, true] {
        let (device, queue) = benchmark_device(portable);
        let mode = if portable { "portable" } else { "native" };
        let mut group = c.benchmark_group(format!("{mode}_root_batches"));
        let mut renderer = Renderer::new(&device, &queue, 1600, 1000, Color::TRANSPARENT);
        for (name, canvas, batches) in benchmark_scenes() {
            let width = canvas.physical_width();
            let height = canvas.physical_height();
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tileink root-batch benchmark target"),
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
            renderer
                .render_to_wgpu_texture(&canvas, &target)
                .expect("benchmark warmup must render");
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("benchmark warmup must complete");
            let stats = renderer.incremental_render_stats();
            assert_eq!(stats.root_draw_batches, batches);
            assert_eq!(stats.portable_texture_copies, if portable { 2 } else { 0 });
            eprintln!(
                "{mode} {width}x{height}/{batches}: root_batches={}, submissions={}, portable_copies={}",
                stats.root_draw_batches, stats.queue_submissions, stats.portable_texture_copies
            );
            group.bench_function(name, |b| {
                b.iter_custom(|iterations| {
                    let start = Instant::now();
                    for _ in 0..iterations {
                        renderer
                            .render_to_wgpu_texture(&canvas, &target)
                            .expect("root-batch frame must render");
                        device
                            .poll(wgpu::PollType::wait_indefinitely())
                            .expect("root-batch frame must complete");
                    }
                    start.elapsed()
                });
            });
        }
        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(4));
    targets = root_batch_submission
}
criterion_main!(benches);
