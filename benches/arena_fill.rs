use std::{hint::black_box, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use tileink::SceneArenaFillBenchmark;

const SIZES: [usize; 4] = [256, 4_096, 65_536, 1_048_576];

fn arena_fill(c: &mut Criterion) {
    let mut insert = c.benchmark_group("arena_fill/insert");
    for len in SIZES {
        insert.throughput(criterion::Throughput::Elements(len as u64));
        insert.bench_function(BenchmarkId::from_parameter(len), |b| {
            b.iter_batched(
                SceneArenaFillBenchmark::empty,
                |mut arena| black_box(arena.insert_zeroed(len)),
                BatchSize::SmallInput,
            );
        });
    }
    insert.finish();

    let mut replace = c.benchmark_group("arena_fill/replace-same-length");
    for len in SIZES {
        replace.throughput(criterion::Throughput::Elements(len as u64));
        replace.bench_function(BenchmarkId::from_parameter(len), |b| {
            let mut arena = SceneArenaFillBenchmark::with_allocation(len);
            b.iter(|| black_box(arena.replace_zeroed(len)));
        });
    }
    replace.finish();

    let mut resize = c.benchmark_group("arena_fill/replace-alternating-size");
    for large in SIZES.into_iter().skip(1) {
        let small = large / 2;
        resize.throughput(criterion::Throughput::Elements(large as u64));
        resize.bench_function(BenchmarkId::new("half-full", large), |b| {
            let mut arena = SceneArenaFillBenchmark::with_allocation(large);
            let mut use_small = true;
            b.iter(|| {
                let len = if use_small { small } else { large };
                use_small = !use_small;
                black_box(arena.replace_zeroed(len))
            });
        });
    }
    resize.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = arena_fill
}
criterion_main!(benches);
