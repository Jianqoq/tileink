use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tileink::{FrameDiffBenchmark, FrameDiffBenchmarkCase};

fn frame_diff(c: &mut Criterion) {
    let cases = [
        ("static", 20_000, FrameDiffBenchmarkCase::Static),
        ("one-revision", 20_000, FrameDiffBenchmarkCase::OneRevision),
        (
            "many-revisions",
            20_000,
            FrameDiffBenchmarkCase::ManyRevisions,
        ),
        (
            "reorder-disjoint",
            20_000,
            FrameDiffBenchmarkCase::ReorderDisjoint,
        ),
        (
            "reorder-overlapping",
            1_024,
            FrameDiffBenchmarkCase::ReorderOverlapping,
        ),
        (
            "insert-remove",
            20_000,
            FrameDiffBenchmarkCase::InsertRemove,
        ),
    ];
    let mut group = c.benchmark_group("frame_diff");
    for (name, count, case) in cases {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new(name, count),
            &(count, case),
            |b, &(count, case)| {
                let mut diff = FrameDiffBenchmark::new(count, case);
                b.iter(|| black_box(diff.diff()));
            },
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = frame_diff
}
criterion_main!(benches);
