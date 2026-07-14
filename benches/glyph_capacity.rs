use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tileink::{GlyphCapacityBenchmark, GlyphCapacityBenchmarkCase};

fn glyph_capacity(c: &mut Criterion) {
    let cases = [
        ("stable", GlyphCapacityBenchmarkCase::Stable),
        ("one-glyph", GlyphCapacityBenchmarkCase::OneGlyph),
        (
            "fragmented-glyphs",
            GlyphCapacityBenchmarkCase::FragmentedGlyphs,
        ),
        ("changed-runs", GlyphCapacityBenchmarkCase::ChangedRuns),
        ("changed-draws", GlyphCapacityBenchmarkCase::ChangedDraws),
        ("mixed", GlyphCapacityBenchmarkCase::Mixed),
    ];
    let mut group = c.benchmark_group("glyph_capacity");
    for (name, case) in cases {
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            let mut update = GlyphCapacityBenchmark::new(case);
            b.iter(|| black_box(update.update()));
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
    targets = glyph_capacity
}
criterion_main!(benches);
