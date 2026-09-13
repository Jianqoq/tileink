use super::retained_bench::{self, BenchConfig, Measurements, MutationPhase};
use super::retained_stress::StressScenario;
use std::time::Duration;

#[derive(Clone, Copy)]
enum MeasurementKind {
    ProductionWall,
    ProfiledWall,
    Component,
}

#[derive(Clone, Copy)]
pub(super) enum StressMetric {
    ProductionWall,
    ProfiledWall,
    Transaction,
    Materialize,
    MaterializePlan,
    MaterializeFrame,
    RootFragment(retained_bench::RootFragmentStage),
}

impl StressMetric {
    pub(super) fn name(self) -> &'static str {
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

    pub(super) fn measure(self, measurements: &Measurements) -> Duration {
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

    pub(super) fn applies_to(self, scenario: StressScenario) -> bool {
        !matches!(self, Self::RootFragment(_))
            || matches!(scenario, StressScenario::ManyRootLayersAddRemove)
    }

    pub(super) fn applies_to_phase(self, phase: MutationPhase) -> bool {
        // The scopes belong to insertion by definition. Their absence in removal
        // is checked inside actual profiled samples and by the explicit presence
        // runner, preserving all logical IDs without bypassing Criterion filters.
        !matches!(self, Self::RootFragment(_)) || matches!(phase, MutationPhase::Insert)
    }

    pub(super) fn profile(self) -> bool {
        !matches!(self.kind(), MeasurementKind::ProductionWall)
    }

    pub(super) fn sample(self, scenario: StressScenario, iterations: u64) -> (u64, BenchConfig) {
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

    pub(super) fn counts(self, scenario: StressScenario) -> &'static [usize] {
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

pub(super) const STRESS_METRICS: [StressMetric; 10] = [
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
