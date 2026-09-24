use criterion::{Criterion, criterion_group, criterion_main};
use std::time::{Duration, Instant};

#[allow(dead_code, unused_imports)]
#[path = "support/backend_comparison.rs"]
mod backend;
#[path = "support/benchmark_evidence.rs"]
mod evidence;
#[path = "support/immediate_workloads.rs"]
mod workloads;

fn compare(c: &mut Criterion) {
    let output = std::path::PathBuf::from(std::env::var_os("TILEINK_COMPARE_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let mut gpu = backend::Gpu::new();
    let mut group = c.benchmark_group("immediate_backend_cycles");
    group
        .sample_size(10)
        .sampling_mode(criterion::SamplingMode::Flat)
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_secs(1));
    for case in workloads::cases() {
        if !evidence::selected(&case.name) {
            continue;
        }
        let targets: Vec<_> = case
            .frames
            .iter()
            .map(|canvas| gpu.target(canvas.physical_width(), canvas.physical_height()))
            .collect();
        let frames: Vec<_> = case.frames.iter().zip(&targets).collect();
        let mut pixels = Vec::new();
        for (phase, (canvas, target)) in frames.iter().enumerate() {
            gpu.render_immediate(canvas, target);
            let image = gpu.image_target(target);
            assert_eq!(
                (image.width, image.height),
                (canvas.physical_width(), canvas.physical_height())
            );
            pixels.push(evidence::capture(&image, &output, &case.name, phase));
        }
        for _ in 0..2 {
            for (canvas, target) in &frames {
                gpu.render_immediate(canvas, target);
            }
        }
        group.throughput(criterion::Throughput::Elements(frames.len() as u64));
        group.bench_function(&case.name, |b| {
            b.iter_custom(|iterations| {
                let start = Instant::now();
                for _ in 0..iterations {
                    for (canvas, target) in &frames {
                        gpu.render_immediate(canvas, target);
                    }
                }
                start.elapsed()
            })
        });
        for _ in 0..4 {
            for (canvas, target) in &frames {
                gpu.render_immediate(canvas, target);
            }
        }
        let mut latency = Vec::new();
        let start = Instant::now();
        for _ in 0..64 / frames.len() {
            for (canvas, target) in &frames {
                let frame_start = Instant::now();
                gpu.render_immediate(canvas, target);
                latency.push(frame_start.elapsed().as_nanos() as u64);
            }
            if latency.len() >= 4 && start.elapsed() >= Duration::from_secs(1) {
                break;
            }
        }
        let profile = std::env::var_os("TILEINK_COMPARE_PROFILE").map(|_| {
            (0..32)
                .map(|index| {
                    let (canvas, target) = frames[index % frames.len()];
                    gpu.profile_immediate(canvas, target)
                })
                .collect::<Vec<_>>()
        });
        std::fs::write(
            output.join(format!("{}.json", case.name)),
            serde_json::to_vec_pretty(
                &serde_json::json!({"name":case.name, "frames_per_iteration":frames.len(),
                "latency_ns":latency, "profile_submit_wait_ns":profile, "pixels":pixels}),
            )
            .unwrap(),
        )
        .unwrap();
    }
    group.finish();
}

criterion_group!(benches, compare);
criterion_main!(benches);
