use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tileink::{Bounds, RetainedMaterializerBenchmark, RetainedNodeId, RetainedScene};

const SIZE: u32 = 4096;

fn tiles_for_bounds(c: &mut Criterion) {
    let root = RetainedNodeId::new(1, 1);
    let scene = RetainedScene::new(SIZE, SIZE, 1.0, root).unwrap();
    let materializer = RetainedMaterializerBenchmark::new(&scene);
    let cases = [
        ("outside", Bounds::new(-64, -64, -1, -1), 0),
        ("single", Bounds::new(5, 7, 12, 15), 1),
        ("clipped-3x3", Bounds::new(-40, -40, 48, 48), 9),
        ("full-row", Bounds::new(0, 512, SIZE as i32, 528), 256),
        ("medium-64x64", Bounds::new(512, 512, 1536, 1536), 4096),
        (
            "full-256x256",
            Bounds::new(0, 0, SIZE as i32, SIZE as i32),
            65_536,
        ),
    ];
    let mut group = c.benchmark_group("tiles_for_bounds");
    for (name, bounds, tiles) in cases {
        group.throughput(Throughput::Elements(tiles));
        group.bench_with_input(BenchmarkId::from_parameter(name), &bounds, |b, &bounds| {
            b.iter(|| black_box(materializer.visit_tiles_for_bounds(black_box(bounds))));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = tiles_for_bounds
}
criterion_main!(benches);
