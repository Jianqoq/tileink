#[path = "../examples/support/retained_bench.rs"]
mod retained_bench;
#[path = "../examples/support/retained_stress.rs"]
mod retained_stress;

use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use peniko::Color;
use retained_bench::{
    BenchConfig, HEIGHT, Measurements, MutationPhase, WIDTH, bench_persistent,
    bench_persistent_phase,
};
use retained_stress::{StressScenario, StressWorkload};
use tileink::{IncrementalRenderMode, WgpuRenderer};

fn retained_stress(c: &mut Criterion) {
    let seed = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT);
    for scenario in StressScenario::ALL {
        for (metric, duration) in [
            (
                "wall",
                (|measurements: &Measurements| measurements.wall.iter().sum::<Duration>())
                    as fn(&Measurements) -> Duration,
            ),
            ("transaction", |measurements| measurements.transaction),
            ("materialize", |measurements| measurements.materialize),
            ("materialize-plan", |measurements| {
                measurements.materialize_plan_sync
            }),
            ("materialize-frame", |measurements| {
                measurements.materialize_frame
            }),
            ("root-fragment-compile", |measurements| {
                measurements.root_fragment_compile
            }),
            ("root-fragment-spatial", |measurements| {
                measurements.root_fragment_spatial
            }),
            ("root-fragment-plan", |measurements| {
                measurements.root_fragment_plan
            }),
            ("root-fragment-metadata", |measurements| {
                measurements.root_fragment_metadata
            }),
        ] {
            let mut group =
                c.benchmark_group(format!("retained_stress/{}/{metric}", scenario.name()));
            for &count in scenario.counts() {
                group.throughput(Throughput::Elements(count as u64));
                group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                    b.iter_custom(|iterations| {
                        let workload = StressWorkload::new(count, scenario);
                        let measurements = bench_persistent(
                            &seed,
                            BenchConfig {
                                // The next rotating mutation crosses RetainedFrameDelta's depth
                                // boundary, so this benchmark permanently measures the cliff.
                                warmup: if matches!(scenario, StressScenario::DeltaRotation) {
                                    255
                                } else {
                                    3
                                },
                                frames: iterations as usize,
                            },
                            workload.build_scene(),
                            IncrementalRenderMode::Auto,
                            |scene, frame| workload.mutate(scene, frame),
                        )
                        .expect("retained stress benchmark must render");
                        duration(&measurements)
                    });
                });
            }
            group.finish();
        }
    }

    let scenario = StressScenario::ManyRootLayersAddRemove;
    for (phase, phase_name) in [
        (MutationPhase::Insert, "insert"),
        (MutationPhase::Remove, "remove"),
    ] {
        for (metric, duration) in [
            (
                "wall",
                (|measurements: &Measurements| measurements.wall.iter().sum::<Duration>())
                    as fn(&Measurements) -> Duration,
            ),
            ("transaction", |measurements| measurements.transaction),
            ("materialize", |measurements| measurements.materialize),
            ("materialize-plan", |measurements| {
                measurements.materialize_plan_sync
            }),
            ("materialize-frame", |measurements| {
                measurements.materialize_frame
            }),
            ("root-fragment-compile", |measurements| {
                measurements.root_fragment_compile
            }),
            ("root-fragment-spatial", |measurements| {
                measurements.root_fragment_spatial
            }),
            ("root-fragment-plan", |measurements| {
                measurements.root_fragment_plan
            }),
            ("root-fragment-metadata", |measurements| {
                measurements.root_fragment_metadata
            }),
        ] {
            let mut group = c.benchmark_group(format!(
                "retained_stress/many-root-layers-{phase_name}/{metric}"
            ));
            for &count in scenario.counts() {
                group.throughput(Throughput::Elements(count as u64));
                group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                    b.iter_custom(|iterations| {
                        let workload = StressWorkload::new(count, scenario);
                        let measurements = bench_persistent_phase(
                            &seed,
                            BenchConfig {
                                warmup: 1,
                                frames: iterations as usize,
                            },
                            workload.build_scene(),
                            IncrementalRenderMode::Auto,
                            phase,
                            |scene, frame| workload.mutate(scene, frame),
                        )
                        .expect("retained root-layer phase benchmark must render");
                        duration(&measurements)
                    });
                });
            }
            group.finish();
        }
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = retained_stress
}
criterion_main!(benches);
