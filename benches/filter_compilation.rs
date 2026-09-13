use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use std::{hint::black_box, time::Duration};
use tileink::FilterCompilationBenchmark;

#[path = "../examples/common/benchmark_gpu.rs"]
mod explicit_gpu;

fn filter_compilation(c: &mut Criterion) {
    let api = std::env::var("TILEINK_BENCH_API").expect("explicit GPU API required");
    let portable = std::env::var("TILEINK_WGPU_MODE").as_deref() == Ok("portable");
    let (_, device, _queue) =
        explicit_gpu::device(&api, portable, false, wgpu::MemoryHints::Performance);
    let mut group = c.benchmark_group("filter_compilation");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
    group.bench_function("first_filter_after_clear", |b| {
        b.iter_batched_ref(
            || FilterCompilationBenchmark::new(&device),
            |workload| assert_eq!(black_box(workload.compile_filter()), 3),
            BatchSize::PerIteration,
        );
    });
    let warm = FilterCompilationBenchmark::new(&device);
    assert_eq!(warm.compile_filter(), 3);
    group.bench_function("cached_filter", |b| {
        b.iter(|| assert_eq!(black_box(warm.compile_filter()), 0));
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().configure_from_args();
    targets = filter_compilation
}
criterion_main!(benches);
