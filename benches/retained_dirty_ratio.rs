#[path = "../examples/support/retained_bench.rs"]
mod retained_bench;
#[path = "../examples/support/retained_dirty_ratio.rs"]
mod retained_dirty_ratio;

use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use peniko::Color;
use retained_bench::{BenchConfig, HEIGHT, WIDTH, bench, bench_persistent};
use retained_dirty_ratio::{BACKGROUND_NODES, RATIOS, Workload};
use tileink::{IncrementalRenderMode, WgpuRenderer};

#[derive(Clone, Copy)]
enum Mode {
    PersistentAuto,
    PersistentForceFull,
    Auto,
    ForceFull,
    LegacyAuto,
    LegacyForceFull,
    Immediate,
}

impl Mode {
    const ALL: [Self; 7] = [
        Self::PersistentAuto,
        Self::PersistentForceFull,
        Self::Auto,
        Self::ForceFull,
        Self::LegacyAuto,
        Self::LegacyForceFull,
        Self::Immediate,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::PersistentAuto => "persistent-auto",
            Self::PersistentForceFull => "persistent-force-full",
            Self::Auto => "auto",
            Self::ForceFull => "force-full",
            Self::LegacyAuto => "legacy-fallback-auto",
            Self::LegacyForceFull => "legacy-fallback-force-full",
            Self::Immediate => "preflat-immediate",
        }
    }
}

fn retained_dirty_ratio(c: &mut Criterion) {
    let seed = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT);
    let workload = Workload::new();
    let mut group = c.benchmark_group("retained_dirty_ratio");
    group.throughput(Throughput::Elements(BACKGROUND_NODES as u64));
    for mode in Mode::ALL {
        for ratio in RATIOS {
            group.bench_with_input(
                BenchmarkId::new(mode.name(), format!("{:.1}%", ratio * 100.0)),
                &ratio,
                |b, &ratio| {
                    b.iter_custom(|iterations| {
                        if matches!(mode, Mode::PersistentAuto | Mode::PersistentForceFull) {
                            let render_mode = if matches!(mode, Mode::PersistentForceFull) {
                                IncrementalRenderMode::ForceFull
                            } else {
                                IncrementalRenderMode::Auto
                            };
                            let measurements = bench_persistent(
                                &seed,
                                BenchConfig {
                                    warmup: 1,
                                    frames: iterations as usize,
                                },
                                workload.persistent_scene(ratio),
                                render_mode,
                                |scene, frame| workload.mutate_persistent(scene, ratio, frame),
                            )
                            .expect("persistent dirty-ratio Criterion benchmark must render");
                            return measurements.wall.into_iter().sum::<Duration>();
                        }
                        let frames = match mode {
                            Mode::Auto | Mode::ForceFull => workload.retained_frames(ratio),
                            Mode::LegacyAuto | Mode::LegacyForceFull => {
                                workload.legacy_retained_frames(ratio)
                            }
                            Mode::Immediate => workload.immediate_frames(ratio),
                            Mode::PersistentAuto | Mode::PersistentForceFull => unreachable!(),
                        };
                        let render_mode = match mode {
                            Mode::ForceFull | Mode::LegacyForceFull => {
                                IncrementalRenderMode::ForceFull
                            }
                            Mode::PersistentAuto
                            | Mode::PersistentForceFull
                            | Mode::Auto
                            | Mode::LegacyAuto
                            | Mode::Immediate => IncrementalRenderMode::Auto,
                        };
                        let measurements = bench(
                            &seed,
                            BenchConfig {
                                warmup: 1,
                                frames: iterations as usize,
                            },
                            &frames,
                            render_mode,
                        )
                        .expect("dirty-ratio Criterion benchmark must render");
                        measurements.wall.into_iter().sum::<Duration>()
                    });
                },
            );
        }
    }
    group.finish();
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
