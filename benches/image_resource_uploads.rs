use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use peniko::Color;
use std::time::Duration;
use tileink::{ImageResourceUploadBenchmark, Renderer};

#[path = "../examples/common/benchmark_gpu.rs"]
mod benchmark_gpu;

fn image_resource_uploads(c: &mut Criterion) {
    let api = std::env::var("TILEINK_BENCH_API").unwrap_or_else(|_| "vulkan".into());
    let (_, device, queue) =
        benchmark_gpu::device(&api, true, false, wgpu::MemoryHints::Performance);
    let renderer = Renderer::new(&device, &queue, 1, 1, Color::TRANSPARENT);
    let mut group = c.benchmark_group("image_atlas_growth_upload");
    for (height, count) in [
        (32, 0),
        (32, 1),
        (32, 4),
        (32, 16),
        (512, 1),
        (512, 4),
        (512, 16),
    ] {
        let workload = ImageResourceUploadBenchmark::new(&renderer, height, count);
        for (name, force_all) in [("incremental", false), ("explicit-full", true)] {
            group.bench_with_input(
                BenchmarkId::new(name, format!("2048x{height}/{count}")),
                &force_all,
                |b, &force_all| {
                    // One prepared allocation at a time bounds GPU memory; setup and
                    // destruction are outside the timed upload/submit/completion.
                    b.iter_batched_ref(
                        || workload.prepare(),
                        |prepared| prepared.upload(force_all),
                        BatchSize::PerIteration,
                    );
                },
            );
        }
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(2));
    targets = image_resource_uploads
}
criterion_main!(benches);
