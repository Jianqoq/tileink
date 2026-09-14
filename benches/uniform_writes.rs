use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::{hint::black_box, time::Duration};
use tileink::UniformWriteBenchmark;

fn uniform_writes(c: &mut Criterion) {
    let mut group = c.benchmark_group("uniform_aggregation");
    for renderers in [1, 2, 8, 32, 128, 512] {
        let workload = UniformWriteBenchmark::new(renderers);
        group.throughput(Throughput::Elements((renderers * 5 * 8) as u64));
        group.bench_with_input(
            BenchmarkId::new("renderers", renderers),
            &workload,
            |b, workload| {
                b.iter(|| black_box(workload.aggregate_frame()));
            },
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = uniform_writes
}
criterion_main!(benches);
