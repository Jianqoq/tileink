#[path = "support/calibration.rs"]
mod calibration;

use retained_bench::benchmark_gpu;
#[path = "../examples/support/retained_bench.rs"]
mod retained_bench;
#[path = "../examples/support/retained_stress.rs"]
mod retained_stress;

use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use retained_bench::{
    BenchConfig, Measurements, MutationPhase, bench_persistent, bench_persistent_phase,
};
use retained_stress::{StressScenario, StressWorkload};
use tileink::IncrementalRenderMode;

#[derive(Clone, Copy)]
enum MeasurementKind {
    ProductionWall,
    ProfiledWall,
    Component,
}

#[derive(Clone, Copy)]
enum StressMetric {
    ProductionWall,
    ProfiledWall,
    Transaction,
    Materialize,
    MaterializePlan,
    MaterializeFrame,
    RootFragment(retained_bench::RootFragmentStage),
}

impl StressMetric {
    fn name(self) -> &'static str {
        match self {
            Self::ProductionWall => "production-wall",
            Self::ProfiledWall => "wall",
            Self::Transaction => "transaction",
            Self::Materialize => "materialize",
            Self::MaterializePlan => "materialize-plan",
            Self::MaterializeFrame => "materialize-frame",
            Self::RootFragment(stage) => stage.name(),
        }
    }

    fn kind(self) -> MeasurementKind {
        match self {
            Self::ProductionWall => MeasurementKind::ProductionWall,
            Self::ProfiledWall => MeasurementKind::ProfiledWall,
            _ => MeasurementKind::Component,
        }
    }

    fn measure(self, measurements: &Measurements) -> Duration {
        match self {
            Self::ProductionWall | Self::ProfiledWall => measurements.wall.iter().sum(),
            Self::Transaction => measurements.transaction,
            Self::Materialize => measurements.materialize,
            Self::MaterializePlan => measurements.materialize_plan_sync,
            Self::MaterializeFrame => measurements.materialize_frame,
            Self::RootFragment(stage) => measurements.root_fragment_stages[stage as usize]
                .duration()
                .expect("a timed root scope needs complete nonzero measurements")
                .expect("an absent scope cannot be timed by Criterion"),
        }
    }

    fn applies_to(self, scenario: StressScenario) -> bool {
        !matches!(self, Self::RootFragment(_))
            || matches!(scenario, StressScenario::ManyRootLayersAddRemove)
    }

    fn applies_to_phase(self, phase: MutationPhase) -> bool {
        // The scopes belong to insertion by definition. Their absence in removal
        // is checked inside actual profiled samples and by the explicit presence
        // runner, preserving all logical IDs without bypassing Criterion filters.
        !matches!(self, Self::RootFragment(_)) || matches!(phase, MutationPhase::Insert)
    }

    fn profile(self) -> bool {
        !matches!(self.kind(), MeasurementKind::ProductionWall)
    }

    fn sample(self, scenario: StressScenario, iterations: u64) -> (u64, BenchConfig) {
        if !self.profile() && matches!(scenario, StressScenario::DeltaRotation) {
            // Node rotation and delta-chain merging have independent periods. Restart each
            // production unit at the same warmed state, so Criterion's iteration count
            // cannot change which merge boundary or node prefix the measurement covers.
            return (
                iterations,
                BenchConfig {
                    warmup: 255,
                    frames: 255,
                    profile: false,
                },
            );
        }
        let warmup = if matches!(scenario, StressScenario::DeltaRotation) {
            255
        } else {
            3
        };
        let mut config = BenchConfig::alternating(warmup, iterations, self.profile());
        if matches!(self, Self::RootFragment(_)) {
            // These insertion scopes do not execute during the removal phase.
            // One Criterion unit includes both phases, even for iterations=1;
            // timing an arbitrary odd prefix would depend on calibration choices.
            config.frames = config
                .frames
                .checked_mul(2)
                .expect("benchmark frame count overflow");
        }
        (1, config)
    }

    fn counts(self, scenario: StressScenario) -> &'static [usize] {
        let counts = scenario.counts();
        // Classify by measurement semantics, never by the display name: every wall
        // series must retain the largest scales, including production insert/remove phases.
        if matches!(self.kind(), MeasurementKind::Component) {
            // Component metrics execute the whole workload but return only one small phase.
            // Large counts make Criterion oversample that hidden setup/render cost.
            &counts[..counts.len().min(3)]
        } else {
            counts
        }
    }
}

const STRESS_METRICS: [StressMetric; 10] = [
    StressMetric::ProductionWall,
    StressMetric::ProfiledWall,
    StressMetric::Transaction,
    StressMetric::Materialize,
    StressMetric::MaterializePlan,
    StressMetric::MaterializeFrame,
    StressMetric::RootFragment(retained_bench::RootFragmentStage::Compile),
    StressMetric::RootFragment(retained_bench::RootFragmentStage::Spatial),
    StressMetric::RootFragment(retained_bench::RootFragmentStage::Plan),
    StressMetric::RootFragment(retained_bench::RootFragmentStage::Metadata),
];

fn retained_stress(c: &mut Criterion) {
    let api = std::env::var("TILEINK_BENCH_API").unwrap_or_else(|_| "vulkan".into());
    let (_, device, queue) =
        benchmark_gpu::device(&api, false, true, wgpu::MemoryHints::MemoryUsage);
    let context = retained_bench::BenchContext::new(&device, &queue);
    for scenario in StressScenario::ALL {
        for metric in STRESS_METRICS
            .into_iter()
            .filter(|metric| metric.applies_to(scenario))
        {
            let mut group = c.benchmark_group(format!(
                "retained_stress/{}/{}",
                scenario.name(),
                metric.name()
            ));
            for &count in metric.counts(scenario) {
                group.throughput(Throughput::Elements(
                    count as u64 * metric.sample(scenario, 1).1.frames as u64,
                ));
                let mut warmed = false;
                group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                    b.iter_custom(calibration::warm_once(&mut warmed, |iterations| {
                        let (repeats, config) = metric.sample(scenario, iterations);
                        (0..repeats)
                            .map(|_| {
                                let workload = StressWorkload::new(count, scenario);
                                let measurements = bench_persistent(
                                    &context,
                                    config,
                                    workload.build_scene(),
                                    IncrementalRenderMode::Auto,
                                    |scene, frame| workload.mutate(scene, frame),
                                )
                                .expect("retained stress benchmark must render");
                                metric.measure(&measurements)
                            })
                            .sum::<Duration>()
                    }));
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
        for metric in STRESS_METRICS
            .into_iter()
            .filter(|metric| metric.applies_to_phase(phase))
        {
            let mut group = c.benchmark_group(format!(
                "retained_stress/many-root-layers-{phase_name}/{}",
                metric.name()
            ));
            for &count in metric.counts(scenario) {
                group.throughput(Throughput::Elements(count as u64));
                let mut warmed = false;
                group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                    b.iter_custom(calibration::warm_once(&mut warmed, |iterations| {
                        let workload = StressWorkload::new(count, scenario);
                        let measurements = bench_persistent_phase(
                            &context,
                            BenchConfig {
                                warmup: 1,
                                frames: iterations as usize,
                                profile: metric.profile(),
                            },
                            workload.build_scene(),
                            IncrementalRenderMode::Auto,
                            phase,
                            |scene, frame| workload.mutate(scene, frame),
                        )
                        .expect("retained root-layer phase benchmark must render");
                        if metric.profile() && matches!(phase, MutationPhase::Remove) {
                            for stage in measurements.root_fragment_stages {
                                assert_eq!(stage.duration().expect("complete timing profile"), None,
                                    "an insertion scope ran in removal; update the scope contract and baseline");
                            }
                        }
                        metric.measure(&measurements)
                    }));
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

#[cfg(test)]
mod tests {
    #[test]
    fn delta_production_repeats_an_identical_merge_boundary_trace() {
        use super::*;
        let metric = STRESS_METRICS
            .iter()
            .find(|metric| metric.name() == "production-wall")
            .unwrap();
        for iterations in [1, 2, 7] {
            let (repeats, config) = metric.sample(StressScenario::DeltaRotation, iterations);
            assert_eq!(repeats, iterations);
            assert_eq!(config.warmup, 255);
            assert_eq!(config.frames, 255);
            assert!(!config.profile);
        }
    }

    #[test]
    fn production_stress_covers_every_wall_scale_and_phase() {
        use super::*;
        let production = STRESS_METRICS
            .iter()
            .find(|metric| metric.name() == "production-wall")
            .unwrap();
        let diagnostic = STRESS_METRICS
            .iter()
            .find(|metric| metric.name() == "wall")
            .unwrap();
        assert!(!production.profile());
        assert!(diagnostic.profile());
        for scenario in StressScenario::ALL {
            assert_eq!(
                production.counts(scenario),
                diagnostic.counts(scenario),
                "{}",
                scenario.name()
            );
        }
        let count = |metric: &StressMetric| {
            StressScenario::ALL
                .into_iter()
                .filter(|&scenario| metric.applies_to(scenario))
                .map(|scenario| metric.counts(scenario).len())
                .sum::<usize>()
                + 2 * metric.counts(StressScenario::ManyRootLayersAddRemove).len()
        };
        assert_eq!(
            count(production),
            40,
            "production series must include both mutation phases"
        );
        assert_eq!(STRESS_METRICS.iter().map(count).sum::<usize>(), 212);
    }
    #[test]
    fn absent_insertion_scopes_keep_logical_coverage_without_numeric_samples() {
        use super::*;
        let scenario = StressScenario::ManyRootLayersAddRemove;
        let skipped = STRESS_METRICS
            .into_iter()
            .filter(|metric| !metric.applies_to_phase(MutationPhase::Remove));
        assert_eq!(
            skipped
                .map(|metric| metric.counts(scenario).len())
                .sum::<usize>(),
            12
        );
        for metric in STRESS_METRICS {
            assert!(metric.applies_to_phase(MutationPhase::Insert));
            assert_eq!(
                metric.applies_to_phase(MutationPhase::Remove),
                !matches!(metric, StressMetric::RootFragment(_))
            );
        }
        let numeric = STRESS_METRICS
            .into_iter()
            .map(|metric| {
                StressScenario::ALL
                    .into_iter()
                    .filter(|&scenario| metric.applies_to(scenario))
                    .map(|scenario| metric.counts(scenario).len())
                    .sum::<usize>()
                    + [MutationPhase::Insert, MutationPhase::Remove]
                        .into_iter()
                        .filter(|&phase| metric.applies_to_phase(phase))
                        .count()
                        * metric.counts(scenario).len()
            })
            .sum::<usize>();
        assert_eq!(numeric, 200);
    }

    #[test]
    fn root_fragment_combined_samples_include_complete_insertion_removal_cycles() {
        use super::*;
        for metric in STRESS_METRICS
            .into_iter()
            .filter(|metric| matches!(metric, StressMetric::RootFragment(_)))
        {
            for iterations in [1, 2, 3, 17] {
                let (repeats, config) =
                    metric.sample(StressScenario::ManyRootLayersAddRemove, iterations);
                assert_eq!(repeats, 1);
                assert!(config.profile);
                let mut phases = [0, 0];
                for frame in config.warmup..config.warmup + config.frames {
                    phases[frame % 2] += 1;
                }
                assert_eq!(phases, [iterations as usize; 2], "{}", metric.name());
            }
        }
    }
}
