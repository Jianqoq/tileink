use criterion::{Criterion, criterion_group, criterion_main};
use std::time::{Duration, Instant};

#[allow(dead_code, unused_imports)]
#[path = "support/backend_comparison.rs"]
mod backend;
#[path = "support/benchmark_evidence.rs"]
mod evidence;

// A multiple of both the geometry period (16) and all burst sizes (1, 2, 3, 4).
const CYCLE: usize = 48;
const BURSTS: [usize; 4] = [1, 2, 3, 4];

fn compare(c: &mut Criterion) {
    let output = std::path::PathBuf::from(std::env::var_os("TILEINK_COMPARE_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let mut gpu = backend::Gpu::new();
    let mut group = c.benchmark_group("pipelined_backend_cycles");
    group
        .sample_size(10)
        .sampling_mode(criterion::SamplingMode::Flat)
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_secs(1))
        .throughput(criterion::Throughput::Elements(CYCLE as u64));
    for scene_name in backend::CASES {
        for burst in BURSTS {
            let name = format!("{scene_name}-burst-{burst}");
            if !evidence::selected(&name) {
                continue;
            }
            let mut workload = backend::Workload::new(scene_name);
            let mut batch = backend::Batch::new(burst);
            let mut pixels = Vec::new();
            // Check every burst boundary in a complete identical content trace.
            // Readbacks are deliberately outside timed throughput measurements.
            for phase in 0..CYCLE / burst {
                batch.render(&mut gpu, &mut workload, burst);
                let image = gpu.image(&mut workload);
                assert_eq!([image.width, image.height], workload.size());
                pixels.push(evidence::capture(&image, &output, &name, phase));
            }
            group.bench_function(&name, |b| {
                b.iter_custom(|iterations| {
                    let start = Instant::now();
                    for _ in 0..iterations {
                        for _ in 0..CYCLE / burst {
                            batch.render(&mut gpu, &mut workload, burst);
                        }
                    }
                    start.elapsed()
                })
            });
            for _ in 0..CYCLE / burst {
                batch.render(&mut gpu, &mut workload, burst);
            }
            let mut latency = Vec::new();
            let start = Instant::now();
            for _ in 0..64 {
                let batch_start = Instant::now();
                batch.render(&mut gpu, &mut workload, burst);
                latency.push(batch_start.elapsed().as_nanos() as u64);
                if latency.len() >= 4 && start.elapsed() >= Duration::from_secs(1) {
                    break;
                }
            }
            std::fs::write(
                output.join(format!("{name}.json")),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"name":name,"frames_per_iteration":CYCLE,
                    "burst":burst,"latency_kind":"completed-burst", "latency_ns":latency,
                    "pixels":pixels}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
    group.finish();
}

criterion_group!(benches, compare);
criterion_main!(benches);
