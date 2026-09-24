#[path = "support/calibration.rs"]
mod calibration;

#[cfg(feature = "bench-internals")]
use peniko::Color;

use retained_bench::benchmark_gpu;
#[path = "../examples/support/retained_bench.rs"]
mod retained_bench;
#[path = "../examples/support/retained_scale.rs"]
mod retained_scale;

#[cfg(feature = "bench-internals")]
use std::rc::Rc;
use std::time::Duration;

#[cfg(feature = "bench-internals")]
use criterion::BatchSize;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
#[cfg(feature = "bench-internals")]
use peniko::kurbo::{Affine, Rect, Shape};
use retained_bench::{
    BenchConfig, Measurements, MutationPhase, bench_persistent, bench_persistent_phase,
};
use retained_scale::{Scenario, Workload};
use tileink::IncrementalRenderMode;
#[cfg(feature = "bench-internals")]
use tileink::{
    Canvas, FillRule, RetainedMaterializerBenchmark, RetainedNodeId, RetainedParent, RetainedScene,
};

const COUNTS: [usize; 4] = [100, 1_000, 5_000, 20_000];
#[cfg(feature = "bench-internals")]
const RAPID_RESIZE_SIZES: [(u32, u32); 8] = [
    (1600, 1000),
    (1568, 982),
    (1536, 964),
    (1504, 946),
    (1472, 928),
    (1504, 946),
    (1536, 964),
    (1568, 982),
];

#[cfg(feature = "bench-internals")]
fn resize_removal_workload() -> (RetainedScene, RetainedMaterializerBenchmark, RetainedNodeId) {
    let path_grid = |count: usize, width: f64| {
        let mut canvas = Canvas::new(512, 32, 1.0);
        for index in 0..count {
            let y = (index % 16) as f64 * 2.0;
            canvas.push_path(
                Rect::new(0.0, y, width, y + 1.0).to_path(0.1),
                Color::WHITE,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
            );
        }
        std::rc::Rc::new(canvas)
    };
    let root = RetainedNodeId::for_owner(200_000);
    let fragmented = RetainedNodeId::for_owner(200_001);
    let survivor = RetainedNodeId::for_owner(200_002);
    let removed = RetainedNodeId::for_owner(200_003);
    let mut scene = RetainedScene::new(32, 32, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            fragmented,
            path_grid(64, 16.0),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            survivor,
            path_grid(1, 16.0),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            removed,
            path_grid(24, 512.0),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = RetainedMaterializerBenchmark::new(&scene);
    scene
        .transaction()
        .remove_subtree(fragmented)
        .commit()
        .unwrap();
    assert!(materializer.update_incremental(&scene));
    (scene, materializer, removed)
}

#[cfg(feature = "bench-internals")]
fn rapid_resize_workload(count: usize) -> (RetainedScene, RetainedMaterializerBenchmark, bool) {
    let root = RetainedNodeId::for_owner(300_000);
    let mut scene = RetainedScene::new(900, 650, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    for index in 0..count {
        let mut canvas = Canvas::new(900, 650, 1.0);
        canvas.push_path(
            Rect::new(8.0, 8.0, 24.0, 24.0).to_path(0.5),
            Color::WHITE,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.5,
        );
        transaction.insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(300_001 + index as u64),
            Rc::new(canvas),
            Affine::IDENTITY,
        );
    }
    transaction.commit().unwrap();
    let materializer = RetainedMaterializerBenchmark::new(&scene);
    (scene, materializer, false)
}

fn retained_materialize_stage(
    c: &mut Criterion,
    context: &retained_bench::BenchContext,
    scenario: Scenario,
    name: &str,
    duration: fn(&Measurements) -> Duration,
) {
    let mut group = c.benchmark_group(format!("retained_materialize_cycles/{name}"));
    for count in COUNTS {
        group.throughput(Throughput::Elements(count as u64 * 2));
        let mut warmed = false;
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter_custom(calibration::warm_once(&mut warmed, |iterations| {
                let workload = Workload::new(count, scenario);
                let measurements = bench_persistent(
                    context,
                    BenchConfig::paired_cycles(3, iterations, true),
                    workload.build_scene(),
                    IncrementalRenderMode::Auto,
                    |scene, frame| workload.mutate(scene, frame),
                )
                .expect("retained materialization benchmark must render");
                duration(&measurements)
            }));
        });
    }
    group.finish();
}

fn retained_scale(c: &mut Criterion) {
    let api = std::env::var("TILEINK_BENCH_API").unwrap_or_else(|_| "vulkan".into());
    let (_, device, queue) =
        benchmark_gpu::device(&api, false, true, wgpu::MemoryHints::MemoryUsage);
    let context = retained_bench::BenchContext::new(&device, &queue);
    for (prefix, profile) in [
        ("retained_scale", true),
        ("retained_scale_production", false),
    ] {
        retained_frame_cycles(c, &context, prefix, profile);
    }

    // Isolate the fixed scene-bound GPU object churn removed by pooling. The retained end-to-end
    // scenarios above also include workload-dependent scratch targets and complete frame cost.
    #[cfg(feature = "bench-internals")]
    {
        // Large retained UIs resize every path chunk but each chunk ID is unique. Keep this
        // permanent CPU benchmark so resize scratch collection cannot silently regress into
        // per-frame hash allocation and rehashing.
        let mut group = c.benchmark_group("retained_scale/materializer-rapid-resize");
        for count in [100, 1_000, 5_000] {
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                let (mut scene, mut materializer, mut expanded) = rapid_resize_workload(count);
                b.iter(|| {
                    expanded = !expanded;
                    let (width, height) = if expanded { (1900, 1250) } else { (900, 650) };
                    scene
                        .transaction()
                        .resize(width, height, 1.0)
                        .commit()
                        .unwrap();
                    std::hint::black_box(materializer.update_incremental(&scene));
                });
            });
        }
        group.finish();

        // A resize can arrive with new content for every retained leaf. Re-encoding should
        // discard old geometry before changing extent, instead of rescan-allocating old paths.
        let mut group = c.benchmark_group("retained_scale/materializer-resize-revisions");
        for count in [100, 1_000] {
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                let (mut scene, mut materializer, mut expanded) = rapid_resize_workload(count);
                let replacement = [Color::WHITE, Color::BLACK].map(|color| {
                    let mut canvas = Canvas::new(1900, 1250, 1.0);
                    canvas.push_path(
                        Rect::new(8.0, 8.0, 24.0, 24.0).to_path(0.5),
                        color,
                        Affine::IDENTITY,
                        FillRule::NonZero,
                        0.5,
                    );
                    Rc::new(canvas)
                });
                b.iter(|| {
                    expanded = !expanded;
                    let (width, height) = if expanded { (1900, 1250) } else { (900, 650) };
                    let mut transaction = scene.transaction();
                    transaction.resize(width, height, 1.0);
                    for index in 0..count {
                        transaction.replace_scene(
                            RetainedNodeId::for_owner(300_001 + index as u64),
                            Rc::clone(&replacement[expanded as usize]),
                        );
                    }
                    transaction.commit().unwrap();
                    std::hint::black_box(materializer.update_incremental(&scene));
                });
            });
        }
        group.finish();

        // Responsive layouts can insert or remove controls in the same transaction that resizes
        // the surface. The output is already a full redraw, so this benchmark guards against
        // rebuilding scene-wide frame and spatial metadata that no resize frame can consume.
        let mut group = c.benchmark_group("retained_scale/materializer-responsive-resize");
        for count in [100, 1_000, 5_000] {
            group.throughput(Throughput::Elements(count as u64));
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                let (mut scene, mut materializer, mut expanded) = rapid_resize_workload(count);
                let responsive = RetainedNodeId::for_owner(399_999);
                let mut canvas = Canvas::new(1900, 1250, 1.0);
                canvas.push_path(
                    Rect::new(8.0, 8.0, 24.0, 24.0).to_path(0.5),
                    Color::WHITE,
                    Affine::IDENTITY,
                    FillRule::NonZero,
                    0.5,
                );
                let canvas = Rc::new(canvas);
                b.iter(|| {
                    expanded = !expanded;
                    let mut transaction = scene.transaction();
                    if expanded {
                        transaction.resize(1900, 1250, 1.0).insert_scene(
                            RetainedParent::content(RetainedNodeId::for_owner(300_000)),
                            None,
                            responsive,
                            canvas.clone(),
                            Affine::IDENTITY,
                        );
                    } else {
                        transaction.resize(900, 650, 1.0).remove_subtree(responsive);
                    }
                    transaction.commit().unwrap();
                    std::hint::black_box(materializer.update_incremental(&scene));
                });
            });
        }
        group.finish();

        // This isolates the texture-allocation churn from a native interactive resize. The first
        // call establishes capacity; measured iterations must reuse it while logical sizes vary.
        let mut group = c.benchmark_group("retained_scale/internal-target-rapid-resize");
        group.bench_function("shrink-expand", |b| {
            let mut renderer = context.renderer();
            renderer.resize_internal_targets_for_benchmark(&RAPID_RESIZE_SIZES);
            b.iter(|| renderer.resize_internal_targets_for_benchmark(&RAPID_RESIZE_SIZES));
        });
        group.finish();

        // Measures the exact materializer phase used by responsive vector UIs: old geometry is
        // removed in the same transaction that expands the viewport. Setup is intentionally
        // untimed so arena compaction and stale-chunk work remain visible in the sample.
        let mut group = c.benchmark_group("retained_scale/materializer-resize-remove");
        group.bench_function("expanded-stale-vector", |b| {
            b.iter_batched(
                resize_removal_workload,
                |(mut scene, mut materializer, removed)| {
                    scene
                        .transaction()
                        .resize(512, 32, 1.0)
                        .remove_subtree(removed)
                        .commit()
                        .unwrap();
                    std::hint::black_box(materializer.update_incremental(&scene));
                },
                BatchSize::SmallInput,
            );
        });
        group.finish();

        let mut group = c.benchmark_group("retained_scale/local-scene-resource-cycle");
        for (policy, reuse) in [("fresh", false), ("pooled", true)] {
            group.bench_function(policy, |b| {
                let mut renderer = context.renderer();
                renderer.set_local_scene_resource_reuse_for_benchmark(reuse);
                renderer.cycle_local_scene_resources_for_benchmark((256, 256));
                b.iter(|| renderer.cycle_local_scene_resources_for_benchmark((256, 256)));
            });
        }
        group.finish();

        let mut group = c.benchmark_group("retained_scale/local-scene-resource-mixed-sizes");
        let sizes = [(128, 128), (320, 192)];
        for (policy, reuse) in [("fresh", false), ("pooled", true)] {
            group.bench_function(policy, |b| {
                let mut renderer = context.renderer();
                renderer.set_local_scene_resource_reuse_for_benchmark(reuse);
                renderer.cycle_mixed_local_scene_resources_for_benchmark(&sizes);
                b.iter(|| renderer.cycle_mixed_local_scene_resources_for_benchmark(&sizes));
            });
        }
        group.finish();
    }

    // The end-to-end groups above intentionally include submit and GPU completion. Keep a second
    // permanent scale series for the dense revision hotspot so CPU materialization regressions are
    // not hidden by GPU scheduling noise. Rendering still executes to preserve real renderer
    // state, but Criterion receives only the profiled retained.materialize duration.
    for (name, duration) in [
        (
            "all-revisions",
            (|m: &Measurements| m.materialize) as fn(&Measurements) -> Duration,
        ),
        ("all-revisions-analysis", |m| m.materialize_analysis),
        ("all-revisions-chunks", |m| m.materialize_chunks),
        ("all-revisions-plan-sync", |m| m.materialize_plan_sync),
        ("all-revisions-frame", |m| m.materialize_frame),
    ] {
        retained_materialize_stage(c, &context, Scenario::AllRevisions, name, duration);
    }
    for (name, duration) in [
        (
            "liquid-glass-move",
            (|m: &Measurements| m.materialize) as fn(&Measurements) -> Duration,
        ),
        ("liquid-glass-move-plan-sync", |m| m.materialize_plan_sync),
        ("liquid-glass-move-frame", |m| m.materialize_frame),
    ] {
        retained_materialize_stage(c, &context, Scenario::LiquidGlassMove, name, duration);
    }
    for (name, duration) in [
        (
            "one-affine",
            (|m: &Measurements| m.materialize) as fn(&Measurements) -> Duration,
        ),
        ("one-affine-chunks", |m| m.materialize_chunks),
        ("one-affine-plan-sync", |m| m.materialize_plan_sync),
        ("one-affine-frame", |m| m.materialize_frame),
    ] {
        retained_materialize_stage(c, &context, Scenario::OneAffine, name, duration);
    }
    for (name, duration) in [
        (
            "affine-clip-update",
            (|m: &Measurements| m.materialize) as fn(&Measurements) -> Duration,
        ),
        ("affine-clip-update-chunks", |m| m.materialize_chunks),
        ("affine-clip-update-plan-sync", |m| m.materialize_plan_sync),
        ("affine-clip-update-frame", |m| m.materialize_frame),
    ] {
        retained_materialize_stage(c, &context, Scenario::AffineClipUpdate, name, duration);
    }
}

fn retained_frame_cycles(
    c: &mut Criterion,
    context: &retained_bench::BenchContext,
    prefix: &str,
    profile: bool,
) {
    for scenario in Scenario::ALL {
        let mut group = c.benchmark_group(format!(
            "{}{}/{}",
            prefix,
            if profile { "_cycles" } else { "" },
            scenario.name()
        ));
        for count in COUNTS {
            group.throughput(Throughput::Elements(count as u64 * 2));
            let mut warmed = false;
            let workload = Workload::new(count, scenario);
            let mut session = None;
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
                b.iter_custom(calibration::warm_once(&mut warmed, |iterations| {
                    let config = BenchConfig::paired_cycles(3, iterations, profile);
                    let session = session.get_or_insert_with(|| {
                        let mut session = retained_bench::PersistentSession::new(
                            context,
                            workload.build_scene(),
                            Default::default(),
                            profile,
                        )
                        .expect("retained benchmark session must initialize");
                        session
                            .warm(config.warmup, |scene, frame| workload.mutate(scene, frame))
                            .expect("retained benchmark session must warm");
                        session
                    });
                    session
                        .measure(config.frames, |scene, frame| workload.mutate(scene, frame))
                        .expect("retained Criterion benchmark must render")
                        .wall
                        .into_iter()
                        .sum::<Duration>()
                }));
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
            let mut group = c.benchmark_group(format!("{prefix}/{name}-{phase_name}"));
            for count in COUNTS {
                group.throughput(Throughput::Elements(count as u64));
                let mut warmed = false;
                group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
                    b.iter_custom(calibration::warm_once(&mut warmed, |iterations| {
                        let workload = Workload::new(count, scenario);
                        let measurements = bench_persistent_phase(
                            context,
                            BenchConfig {
                                warmup: 1,
                                frames: iterations as usize,
                                profile,
                            },
                            workload.build_scene(),
                            IncrementalRenderMode::Auto,
                            phase,
                            |scene, frame| workload.mutate(scene, frame),
                        )
                        .expect("retained phase Criterion benchmark must render");
                        measurements.wall.into_iter().sum::<Duration>()
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
    targets = retained_scale
}
criterion_main!(benches);

#[path = "../examples/support/retained_dimensions.rs"]
mod retained_dimensions;
