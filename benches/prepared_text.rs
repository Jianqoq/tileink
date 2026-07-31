use std::{hint::black_box, time::Duration};

use criterion::{Criterion, criterion_group, criterion_main};
use tileink::PreparedTextBenchmark;

const LABELS: usize = 120;

fn prepared_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("prepared_text");
    group.sample_size(50);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(4));

    group.bench_function("rebuild", |b| {
        let mut workload = PreparedTextBenchmark::new(LABELS);
        b.iter(|| black_box(workload.rebuild()));
    });
    group.bench_function("position_only", |b| {
        let mut workload = PreparedTextBenchmark::new(LABELS);
        b.iter(|| black_box(workload.prepare_position_shift()));
    });
    group.bench_function("glyph_replacement", |b| {
        let mut workload = PreparedTextBenchmark::new(LABELS);
        b.iter(|| black_box(workload.prepare_glyph_replacement()));
    });
    group.bench_function("run_topology", |b| {
        let mut workload = PreparedTextBenchmark::new(LABELS);
        b.iter(|| black_box(workload.prepare_run_topology()));
    });
    group.bench_function("raster_option", |b| {
        let mut workload = PreparedTextBenchmark::new(LABELS);
        b.iter(|| black_box(workload.prepare_raster_option_change()));
    });
    group.bench_function("font_cache_clear", |b| {
        let mut workload = PreparedTextBenchmark::new(LABELS);
        b.iter(|| black_box(workload.prepare_font_cache_clear()));
    });
    group.finish();
}

criterion_group!(benches, prepared_text);
criterion_main!(benches);
