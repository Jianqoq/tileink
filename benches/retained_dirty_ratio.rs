#[path = "support/calibration.rs"]
mod calibration;

use retained_bench::benchmark_gpu;
#[path = "../examples/support/retained_bench.rs"]
mod retained_bench;
#[path = "../examples/support/retained_dirty_ratio.rs"]
mod retained_dirty_ratio;

use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use retained_bench::{BenchConfig, bench, bench_persistent};
use retained_dirty_ratio::{BACKGROUND_NODES, RATIOS, Workload};
use tileink::IncrementalRenderMode;

#[derive(Clone, Copy)]
enum Mode {
    PersistentAuto,
    PersistentForceFull,
    Immediate,
}

impl Mode {
    const ALL: [Self; 3] = [
        Self::PersistentAuto,
        Self::PersistentForceFull,
        Self::Immediate,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::PersistentAuto => "persistent-auto",
            Self::PersistentForceFull => "persistent-force-full",
            Self::Immediate => "preflat-immediate",
        }
    }
}

fn retained_dirty_ratio(c: &mut Criterion) {
    let api = std::env::var("TILEINK_BENCH_API").unwrap_or_else(|_| "vulkan".into());
    let (_, device, queue) =
        benchmark_gpu::device(&api, false, true, wgpu::MemoryHints::MemoryUsage);
    let context = retained_bench::BenchContext::new(&device, &queue);
    let workload = Workload::new();
    for (prefix, profile) in [
        ("retained_dirty_ratio_cycles", true),
        ("retained_dirty_ratio_production", false),
    ] {
        let mut group = c.benchmark_group(prefix);
        group.throughput(Throughput::Elements(BACKGROUND_NODES as u64 * 2));
        for mode in Mode::ALL {
            for ratio in RATIOS {
                let mut warmed = false;
                group.bench_with_input(
                    BenchmarkId::new(mode.name(), format!("{:.1}%", ratio * 100.0)),
                    &ratio,
                    |b, &ratio| {
                        b.iter_custom(calibration::warm_once(&mut warmed, |iterations| {
                            if matches!(mode, Mode::PersistentAuto | Mode::PersistentForceFull) {
                                let render_mode = if matches!(mode, Mode::PersistentForceFull) {
                                    IncrementalRenderMode::ForceFull
                                } else {
                                    IncrementalRenderMode::Auto
                                };
                                let measurements = bench_persistent(
                                    &context,
                                    BenchConfig::paired_cycles(1, iterations, profile),
                                    workload.persistent_scene(ratio),
                                    render_mode,
                                    |scene, frame| workload.mutate_persistent(scene, ratio, frame),
                                )
                                .expect("persistent dirty-ratio Criterion benchmark must render");
                                return measurements.wall.into_iter().sum::<Duration>();
                            }
                            let frames = workload.immediate_frames(ratio);
                            let measurements = bench(
                                &context,
                                BenchConfig::paired_cycles(1, iterations, profile),
                                &frames,
                                IncrementalRenderMode::Auto,
                            )
                            .expect("dirty-ratio Criterion benchmark must render");
                            measurements.wall.into_iter().sum::<Duration>()
                        }));
                    },
                );
            }
        }
        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = retained_dirty_ratio
}
criterion_main!(benches);
