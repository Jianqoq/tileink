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

#[derive(Clone, Copy)]
struct StressMetric {
    name: &'static str,
    duration: fn(&Measurements) -> Duration,
    root_fragment_only: bool,
}

impl StressMetric {
    fn applies_to(self, scenario: StressScenario) -> bool {
        !self.root_fragment_only || matches!(scenario, StressScenario::ManyRootLayersAddRemove)
    }

    fn counts(self, scenario: StressScenario) -> &'static [usize] {
        let counts = scenario.counts();
        if self.name == "wall" {
            counts
        } else {
            // Component metrics still execute the whole workload but return only one small phase.
            // Large counts make Criterion drastically oversample that hidden setup/render cost.
            &counts[..counts.len().min(3)]
        }
    }
}

const STRESS_METRICS: [StressMetric; 9] = [
    StressMetric {
        name: "wall",
        duration: |measurements| measurements.wall.iter().sum(),
        root_fragment_only: false,
    },
    StressMetric {
        name: "transaction",
        duration: |measurements| measurements.transaction,
        root_fragment_only: false,
    },
    StressMetric {
        name: "materialize",
        duration: |measurements| measurements.materialize,
        root_fragment_only: false,
    },
    StressMetric {
        name: "materialize-plan",
        duration: |measurements| measurements.materialize_plan_sync,
        root_fragment_only: false,
    },
    StressMetric {
        name: "materialize-frame",
        duration: |measurements| measurements.materialize_frame,
        root_fragment_only: false,
    },
    StressMetric {
        name: "root-fragment-compile",
        duration: |measurements| measurements.root_fragment_compile,
        root_fragment_only: true,
    },
    StressMetric {
        name: "root-fragment-spatial",
        duration: |measurements| measurements.root_fragment_spatial,
        root_fragment_only: true,
    },
    StressMetric {
        name: "root-fragment-plan",
        duration: |measurements| measurements.root_fragment_plan,
        root_fragment_only: true,
    },
    StressMetric {
        name: "root-fragment-metadata",
        duration: |measurements| measurements.root_fragment_metadata,
        root_fragment_only: true,
    },
];

fn retained_stress(c: &mut Criterion) {
    let seed = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT);
    for scenario in StressScenario::ALL {
        for metric in STRESS_METRICS
            .into_iter()
            .filter(|metric| metric.applies_to(scenario))
        {
            let mut group = c.benchmark_group(format!(
                "retained_stress/{}/{}",
                scenario.name(),
                metric.name
            ));
            for &count in metric.counts(scenario) {
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
                        (metric.duration)(&measurements)
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
        for metric in STRESS_METRICS {
            let mut group = c.benchmark_group(format!(
                "retained_stress/many-root-layers-{phase_name}/{}",
                metric.name
            ));
            for &count in metric.counts(scenario) {
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
                        (metric.duration)(&measurements)
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
