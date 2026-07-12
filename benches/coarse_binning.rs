#[path = "../examples/support/coarse_binning.rs"]
mod coarse_binning;
#[path = "../examples/support/retained_bench.rs"]
mod retained_bench;

use std::time::Duration;

use coarse_binning::{Case, Workload};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use peniko::Color;
use retained_bench::{BenchConfig, HEIGHT, WIDTH, bench_persistent_with_config};
use tileink::{CoarseBinningMode, IncrementalRenderConfig, WgpuRenderer};

fn coarse_binning(c: &mut Criterion) {
    let seed = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT);
    for case in Case::ALL {
        let mut group = c.benchmark_group(format!("coarse_binning/{}", case.name()));
        for (name, mode) in [
            ("auto", CoarseBinningMode::Auto),
            ("compact", CoarseBinningMode::ForceCompact),
            ("dense", CoarseBinningMode::ForceDense),
        ] {
            group.bench_with_input(BenchmarkId::from_parameter(name), &mode, |b, &mode| {
                b.iter_custom(|iterations| {
                    let workload = Workload::new(case);
                    let moving = workload.moving();
                    let config = IncrementalRenderConfig {
                        coarse_binning: mode,
                        ..Default::default()
                    };
                    let measurements = bench_persistent_with_config(
                        &seed,
                        BenchConfig {
                            warmup: 3,
                            frames: iterations as usize,
                        },
                        workload.scene,
                        config,
                        |scene, frame| Workload::mutate(moving, scene, frame),
                    )
                    .expect("coarse-binning Criterion benchmark must render");
                    if measurements.coarse_gpu.is_zero() {
                        measurements.wall.into_iter().sum()
                    } else {
                        measurements.coarse_gpu
                    }
                });
            });
        }
        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = coarse_binning
}
criterion_main!(benches);
