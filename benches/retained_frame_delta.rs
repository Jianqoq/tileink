//! CPU regression coverage for retained delta payload ownership.
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use tileink::RetainedMaterializerBenchmark;

// Reuse the full retained benchmark's scene and mutation semantics without GPU setup.
mod retained_bench {
    pub const WIDTH: u32 = 1024;
    pub const HEIGHT: u32 = 1024;
}
#[allow(dead_code)]
#[path = "../examples/support/retained_scale.rs"]
mod retained_scale;

fn retained_frame_delta(c: &mut Criterion) {
    use retained_scale::{Scenario, Workload};
    let mut group = c.benchmark_group("retained_frame_delta");
    group.throughput(Throughput::Elements(2));
    for (name, scenario) in [
        ("one-affine/100", Scenario::OneAffine),
        ("manual-invalidation/100", Scenario::ManualInvalidation),
    ] {
        group.bench_function(name, |b| {
            let workload = Workload::new(100, scenario);
            let mut scene = workload.build_scene();
            let mut materializer = RetainedMaterializerBenchmark::new(&scene);
            for frame in 0..3 {
                workload.mutate(&mut scene, frame);
                black_box(materializer.update_incremental(&scene));
            }
            let mut frame = 3;
            b.iter(|| {
                // Keep both alternating affine phases in every Criterion iteration.
                for _ in 0..2 {
                    workload.mutate(&mut scene, frame);
                    black_box(materializer.update_incremental(&scene));
                    frame += 1;
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, retained_frame_delta);
criterion_main!(benches);
