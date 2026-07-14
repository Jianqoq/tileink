use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tileink::{GpuDirtyRangesBenchmark, SceneArenaDirtyBenchmark};

const COUNTS: [usize; 3] = [1, 64, 4_096];

fn dirty_ranges(c: &mut Criterion) {
    for (name, overlapping) in [("disjoint", false), ("overlapping", true)] {
        let mut group = c.benchmark_group(format!("dirty_ranges/scene_arena/{name}"));
        for count in COUNTS {
            group.throughput(Throughput::Elements(count as u64));
            group.bench_function(BenchmarkId::from_parameter(count), |b| {
                let mut ranges = SceneArenaDirtyBenchmark::new();
                b.iter(|| black_box(ranges.collect(count, overlapping)));
            });
        }
        group.finish();
    }

    let mut paths = c.benchmark_group("dirty_ranges/path_plan/take");
    for count in COUNTS {
        paths.throughput(Throughput::Elements((count * 4) as u64));
        paths.bench_function(BenchmarkId::from_parameter(count), |b| {
            let mut ranges = GpuDirtyRangesBenchmark::new();
            b.iter(|| black_box(ranges.path_plan_cycle(count)));
        });
    }
    paths.finish();

    let mut bins = c.benchmark_group("dirty_ranges/tile_bins/take");
    for count in COUNTS {
        bins.throughput(Throughput::Elements((count * 2) as u64));
        bins.bench_function(BenchmarkId::from_parameter(count), |b| {
            let mut ranges = GpuDirtyRangesBenchmark::new();
            b.iter(|| black_box(ranges.tile_bin_cycle(count)));
        });
    }
    bins.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = dirty_ranges
}
criterion_main!(benches);
