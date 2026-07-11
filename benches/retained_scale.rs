#[path = "../examples/support/retained_bench.rs"]
mod retained_bench;
#[path = "../examples/support/retained_scale.rs"]
mod retained_scale;

use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use peniko::Color;
use retained_bench::{
    BenchConfig, HEIGHT, MutationPhase, WIDTH, bench_persistent, bench_persistent_phase,
};
use retained_scale::{Scenario, Workload};
use tileink::{IncrementalRenderMode, WgpuRenderer};

const COUNTS: [usize; 5] = [100, 1_000, 5_000, 20_000, 100_000];

fn retained_scale(c: &mut Criterion) {
    let seed = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT);
    for scenario in Scenario::ALL {
        let mut group = c.benchmark_group(format!("retained_scale/{}", scenario.name()));
        for count in COUNTS {
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                b.iter_custom(|iterations| {
                    let workload = Workload::new(count, scenario);
                    let scene = workload.build_scene();
                    let measurements = bench_persistent(
                        &seed,
                        BenchConfig {
                            // Exercise both sides of alternating workloads before Criterion
                            // samples. One warmup frame leaves variable-length/add-remove cases
                            // measuring first-use arena growth in every newly constructed sample.
                            warmup: 3,
                            frames: iterations as usize,
                        },
                        scene,
                        IncrementalRenderMode::Auto,
                        |scene, frame| workload.mutate(scene, frame),
                    )
                    .expect("retained Criterion benchmark must render");
                    measurements.wall.into_iter().sum::<Duration>()
                });
            });
        }
        group.finish();
    }

    for (scenario, name) in [
        (Scenario::LayerAddRemove, "tail-layer"),
        (Scenario::MiddleLayerAddRemove, "middle-layer"),
        (Scenario::NestedLayerAddRemove, "nested-layer"),
    ] {
        for (phase, phase_name) in [
            (MutationPhase::Insert, "insert"),
            (MutationPhase::Remove, "remove"),
        ] {
            let mut group = c.benchmark_group(format!("retained_scale/{name}-{phase_name}"));
            for count in COUNTS {
                group.throughput(Throughput::Elements(count as u64));
                group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                    b.iter_custom(|iterations| {
                        let workload = Workload::new(count, scenario);
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
                        .expect("retained phase Criterion benchmark must render");
                        measurements.wall.into_iter().sum::<Duration>()
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
    targets = retained_scale
}
criterion_main!(benches);
