#[path = "support/retained_bench.rs"]
mod retained_bench;
#[path = "support/retained_presence.rs"]
mod retained_presence;
#[allow(dead_code)]
#[path = "support/retained_stress.rs"]
mod retained_stress;

fn main() {
    use retained_bench::{BenchContext, MutationPhase, benchmark_gpu};
    let frames = std::env::var("TILEINK_BENCH_OBSERVATION_FRAMES")
        .map_or(Ok(retained_presence::OBSERVATION_FRAMES), |value| {
            value.parse::<usize>()
        })
        .expect("observation frame count must be a positive integer");
    let api = std::env::var("TILEINK_BENCH_API").expect("select an explicit benchmark API");
    let (_, device, queue) =
        benchmark_gpu::device(&api, false, true, wgpu::MemoryHints::MemoryUsage);
    let context = BenchContext::new(&device, &queue);
    let counts = retained_stress::StressScenario::ManyRootLayersAddRemove.counts();
    for phase in [MutationPhase::Insert, MutationPhase::Remove] {
        let observations = retained_presence::observe_root_phase(
            &context,
            &counts[..counts.len().min(3)],
            phase,
            frames,
        );
        for stages in observations {
            for stage in stages {
                let measured = stage
                    .duration()
                    .expect("complete stage observation")
                    .is_some();
                assert_eq!(
                    measured,
                    matches!(phase, MutationPhase::Insert),
                    "root fragment scopes currently belong to insertion"
                );
            }
        }
    }
}

#[path = "support/retained_dimensions.rs"]
mod retained_dimensions;
