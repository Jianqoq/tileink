// Reuse the retained inputs and mutations independently of the timer.
#[allow(dead_code)]
#[path = "../../examples/support/retained_dirty_ratio.rs"]
pub mod dirty;
#[allow(dead_code)]
#[path = "../../examples/support/retained_scale.rs"]
pub mod scale;
#[allow(dead_code)]
#[path = "../../examples/support/retained_stress.rs"]
pub mod stress;

pub const COUNTS: [usize; 4] = [100, 1_000, 5_000, 20_000];

#[derive(Clone, Copy)]
pub enum Case {
    Scale(scale::Scenario, usize),
    Dirty(f64),
    DirtyFull(f64),
    Stress(stress::StressScenario, usize),
}

impl Case {
    pub fn all() -> Vec<Self> {
        scale::Scenario::ALL
            .into_iter()
            .flat_map(|scenario| COUNTS.map(|count| Self::Scale(scenario, count)))
            .chain(dirty::RATIOS.map(Self::Dirty))
            .chain(dirty::RATIOS.map(Self::DirtyFull))
            .chain(
                stress::StressScenario::ALL
                    .into_iter()
                    .flat_map(|scenario| {
                        scenario
                            .counts()
                            .iter()
                            .map(move |&count| Self::Stress(scenario, count))
                    }),
            )
            .collect()
    }

    pub fn name(self) -> String {
        match self {
            Self::Scale(scenario, count) => format!("scale-{}-{count}", scenario.name()),
            Self::DirtyFull(ratio) => format!("forcefull-dirty-{:.1}pct", ratio * 100.0),
            Self::Dirty(ratio) => format!("dirty-{:.1}pct", ratio * 100.0),
            Self::Stress(scenario, count) => format!("stress-{}-{count}", scenario.name()),
        }
    }

    pub fn mode(self) -> tileink::IncrementalRenderMode {
        if matches!(self, Self::DirtyFull(_)) {
            tileink::IncrementalRenderMode::ForceFull
        } else {
            tileink::IncrementalRenderMode::Auto
        }
    }

    pub fn rotating(self) -> bool {
        matches!(self, Self::Stress(stress::StressScenario::DeltaRotation, _))
    }

    pub fn workload(self) -> Workload {
        match self {
            Self::Scale(scenario, count) => Workload::Scale(scale::Workload::new(count, scenario)),
            Self::Dirty(ratio) | Self::DirtyFull(ratio) => {
                Workload::Dirty(dirty::Workload::new(), ratio)
            }
            Self::Stress(scenario, count) => {
                Workload::Stress(stress::StressWorkload::new(count, scenario))
            }
        }
    }
}

pub enum Workload {
    Scale(scale::Workload),
    Dirty(dirty::Workload, f64),
    Stress(stress::StressWorkload),
}

impl Workload {
    pub fn scene(&self) -> tileink::RetainedScene {
        match self {
            Self::Scale(workload) => workload.build_scene(),
            Self::Dirty(workload, ratio) => workload.persistent_scene(*ratio),
            Self::Stress(workload) => workload.build_scene(),
        }
    }

    pub fn mutate(&self, scene: &mut tileink::RetainedScene, frame: usize) {
        match self {
            Self::Scale(workload) => workload.mutate(scene, frame),
            Self::Dirty(workload, ratio) => workload.mutate_persistent(scene, *ratio, frame),
            Self::Stress(workload) => workload.mutate(scene, frame),
        }
    }
}

#[path = "../../examples/support/retained_dimensions.rs"]
mod retained_dimensions;
