use std::{hint::black_box, ops::Range, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tileink::TileDrawBinsBenchmark;

struct Case {
    name: &'static str,
    draw_count: usize,
    changed: Vec<Range<usize>>,
    changed_draw_count: u64,
    spatial_change: bool,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "stable-bounds-1",
            draw_count: 16_384,
            changed: std::iter::once(8_192..8_193).collect(),
            changed_draw_count: 1,
            spatial_change: false,
        },
        Case {
            name: "move-contiguous-64",
            draw_count: 16_384,
            changed: std::iter::once(8_000..8_064).collect(),
            changed_draw_count: 64,
            spatial_change: true,
        },
        Case {
            name: "move-overlapping-ranges",
            draw_count: 16_384,
            changed: (0..64)
                .map(|index| 8_000 + index * 4..8_032 + index * 4)
                .collect(),
            changed_draw_count: 284,
            spatial_change: true,
        },
        Case {
            name: "move-contiguous-1024",
            draw_count: 16_384,
            changed: std::iter::once(8_000..9_024).collect(),
            changed_draw_count: 1_024,
            spatial_change: true,
        },
    ]
}

fn tile_draw_bins(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile_draw_bins/update_changed");
    for case in cases() {
        group.throughput(Throughput::Elements(case.changed_draw_count));
        group.bench_with_input(BenchmarkId::from_parameter(case.name), &case, |b, case| {
            let mut bins = TileDrawBinsBenchmark::new(
                case.draw_count,
                case.changed.clone(),
                case.spatial_change,
            );
            b.iter(|| black_box(bins.update()));
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
    targets = tile_draw_bins
}
criterion_main!(benches);
