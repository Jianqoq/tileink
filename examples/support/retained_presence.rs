use super::{
    retained_bench::{
        BenchConfig, BenchContext, MutationPhase, RootFragmentStage, bench_persistent_phase,
        profile_observation::StageObservation,
    },
    retained_stress::{StressScenario, StressWorkload},
};
use tileink::IncrementalRenderMode;

pub const OBSERVATION_FRAMES: usize = 20;

/// Produce independent profiled evidence for every logical root-stage case ID.
/// This explicit runner is separate from Criterion selection; an absent stage
/// has presence evidence and an empty duration, never a numeric timing sample.
pub fn observe_root_phase(
    context: &BenchContext,
    counts: &[usize],
    phase: MutationPhase,
    frames: usize,
) -> Vec<[StageObservation; 4]> {
    assert!(frames > 0, "presence observation requires profiled frames");
    let phase_name = match phase {
        MutationPhase::Insert => "insert",
        MutationPhase::Remove => "remove",
    };
    counts.iter().map(|&count| {
        let workload = StressWorkload::new(count, StressScenario::ManyRootLayersAddRemove);
        let measurements = bench_persistent_phase(
            context,
            BenchConfig { warmup: 1, frames, profile: true },
            workload.build_scene(),
            IncrementalRenderMode::Auto,
            phase,
            |scene, frame| workload.mutate(scene, frame),
        ).expect("root-phase presence probe must render");
        for (observation, stage) in measurements.root_fragment_stages.iter().zip(RootFragmentStage::ALL) {
            let metric = stage.name();
            let scope = stage.scope();
            assert_eq!(observation.profiled_frames, frames as u64);
            let duration = observation.duration().expect("incomplete/zero scope timing is not absence");
            eprintln!("benchmark observation: {}", serde_json::json!({
                "case": format!("retained_stress/many-root-layers-{phase_name}/{metric}/{count}"),
                "scope": scope,
                "state": format!("{:?}", observation.state()),
                "profiled_frames": observation.profiled_frames,
                "entry_count": observation.entry_count,
                "timed_entries": observation.timed_entries,
                "elapsed_ns": duration.map(|duration| duration.as_nanos()),
                "reason": if duration.is_none() { Some("scope did not execute in the measured phase; surrounding operations still take time") } else { None },
            }));
        }
        measurements.root_fragment_stages
    }).collect()
}
