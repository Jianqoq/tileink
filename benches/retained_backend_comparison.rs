#[path = "support/benchmark_evidence.rs"]
mod evidence;
use criterion::{Criterion, criterion_group, criterion_main};
use std::{
    path::Path,
    time::{Duration, Instant},
};

// The adapter also serves the original 11-case and clip suites.
#[allow(dead_code, unused_imports)]
#[path = "support/backend_comparison.rs"]
mod backend;
#[path = "support/retained_comparison_cases.rs"]
mod cases;

fn step(
    gpu: &mut backend::Gpu,
    workload: &cases::Workload,
    scene: &mut backend::Workload,
    frame: &mut usize,
) {
    workload.mutate(&mut scene.scene, *frame);
    gpu.render(scene);
    *frame += 1;
}

fn capture(
    gpu: &mut backend::Gpu,
    scene: &mut backend::Workload,
    output: &Path,
    name: &str,
    phase: usize,
) -> String {
    let image = gpu.image(scene);
    assert_eq!([image.width, image.height], scene.size());
    evidence::capture(&image, output, name, phase)
}

fn compare(c: &mut Criterion) {
    let output = std::path::PathBuf::from(std::env::var_os("TILEINK_COMPARE_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let mut gpu = backend::Gpu::new();
    let mut group = c.benchmark_group("retained_backend_cycles");
    group
        .sample_size(10)
        .sampling_mode(criterion::SamplingMode::Flat)
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_secs(1));
    for case in cases::Case::all() {
        let name = case.name();
        if !evidence::selected(&name) {
            continue;
        }
        gpu.set_incremental_mode(case.mode());
        let workload = case.workload();
        let mut scene = backend::Workload::from_scene(workload.scene());
        let mut frame = 0;
        let mut pixels = Vec::new();
        // Check the initial render and both alternating mutations before warmup.
        gpu.render(&mut scene);
        pixels.push(capture(&mut gpu, &mut scene, &output, &name, frame));
        for _ in 0..2 {
            step(&mut gpu, &workload, &mut scene, &mut frame);
            pixels.push(capture(&mut gpu, &mut scene, &output, &name, frame));
        }
        let frames = if case.rotating() { 255 } else { 2 };
        group.throughput(criterion::Throughput::Elements(frames));
        for _ in 0..4 {
            step(&mut gpu, &workload, &mut scene, &mut frame);
        }
        group.bench_function(&name, |b| {
            b.iter_custom(|iterations| {
                let mut elapsed = Duration::ZERO;
                for _ in 0..iterations {
                    if case.rotating() {
                        // Match the legacy delta benchmark: each sample repeats the same
                        // 255 warm + 255 measured mutation trace, independent of calibration.
                        scene = backend::Workload::from_scene(workload.scene());
                        frame = 0;
                        for _ in 0..255 {
                            step(&mut gpu, &workload, &mut scene, &mut frame);
                        }
                    }
                    let start = Instant::now();
                    for _ in 0..frames {
                        step(&mut gpu, &workload, &mut scene, &mut frame);
                    }
                    elapsed += start.elapsed();
                }
                elapsed
            })
        });
        if case.rotating() {
            scene = backend::Workload::from_scene(workload.scene());
            frame = 0;
            for _ in 0..255 {
                step(&mut gpu, &workload, &mut scene, &mut frame);
            }
        } else {
            for _ in 0..4 {
                step(&mut gpu, &workload, &mut scene, &mut frame);
            }
        }
        let mut latency = Vec::new();
        let latency_start = Instant::now();
        for _ in 0..if case.rotating() { 255 } else { 64 } {
            let start = Instant::now();
            step(&mut gpu, &workload, &mut scene, &mut frame);
            latency.push(start.elapsed().as_nanos() as u64);
            // Keep complete alternating pairs. Extreme stress scenes can take
            // seconds per frame; a coverage sweep must not pretend four samples
            // estimate a tail percentile. The collector leaves P95 absent below 20.
            if !case.rotating()
                && latency.len() >= 4
                && latency.len().is_multiple_of(2)
                && latency_start.elapsed() >= Duration::from_secs(1)
            {
                break;
            }
        }
        if case.rotating() {
            pixels.push(capture(&mut gpu, &mut scene, &output, &name, 510));
        }
        std::fs::write(
            output.join(format!("{name}.json")),
            serde_json::to_vec_pretty(
                &serde_json::json!({"name":name,"frames_per_iteration":frames,
                "latency_ns":latency,"pixels":pixels}),
            )
            .unwrap(),
        )
        .unwrap();
    }
    group.finish();
}

criterion_group!(benches, compare);
criterion_main!(benches);
