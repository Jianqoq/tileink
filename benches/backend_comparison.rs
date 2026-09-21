//! Identical retained workloads across separately compiled backend features.
#[path = "support/backend_comparison.rs"]
mod backend_comparison_support;

use backend_comparison_support::{Gpu, Workload};
use criterion::{Criterion, criterion_group, criterion_main};
use std::time::{Duration, Instant};

fn compare(c: &mut Criterion) {
    let output = std::path::PathBuf::from(
        std::env::var("TILEINK_COMPARE_OUTPUT").expect("set a fresh evidence directory"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let mut group = c.benchmark_group("backend_comparison_cycles");
    group
        .throughput(criterion::Throughput::Elements(16))
        .sample_size(30)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2));
    let mut gpu = Gpu::new();
    let profile = std::env::var_os("TILEINK_COMPARE_PROFILE").is_some();
    for name in backend_comparison_support::CASES {
        if std::env::var("TILEINK_COMPARE_CASE").is_ok_and(|selected| selected != name) {
            continue;
        }
        let mut workload = Workload::new(name);
        // Verify every phase outside timing; preserve raw bytes for four-way comparison.
        for phase in 0..16 {
            workload.advance();
            gpu.render(&mut workload);
            let image = gpu.image(&mut workload);
            assert_eq!([image.width, image.height], workload.size());
            assert!(
                image.pixels.iter().any(|pixel| pixel >> 24 != 0),
                "empty benchmark image: {name}"
            );
            let path = output.join(format!(
                "{name}-{phase}-{}x{}.rgba",
                image.width, image.height
            ));
            assert!(!path.exists(), "evidence must not overwrite an earlier run");
            std::fs::write(path, bytemuck::cast_slice::<u32, u8>(&image.pixels)).unwrap();
        }
        if !profile {
            group.bench_function(name, |b| {
                b.iter_custom(|iterations| {
                    let start = Instant::now();
                    for _ in 0..iterations {
                        // Complete geometry cycles; image replacement content stays fresh.
                        for _ in 0..16 {
                            workload.advance();
                            gpu.render(&mut workload);
                        }
                    }
                    start.elapsed()
                })
            });
        }
        // Individual completed-frame latency is separate from Criterion's aggregated estimates.
        // Rewarm after Criterion analysis, outside the steady-state latency samples.
        for _ in 0..64 {
            workload.advance();
            gpu.render(&mut workload);
        }
        let mut times = Vec::with_capacity(400);
        let mut stages = Vec::new();
        for _ in 0..400 {
            let start = Instant::now();
            workload.advance();
            if profile {
                stages.push(gpu.profile(&mut workload));
            } else {
                gpu.render(&mut workload);
            }
            times.push(start.elapsed().as_nanos() as u64);
        }
        std::fs::write(
            output.join(format!("{name}-latency.json")),
            serde_json::to_vec(&times).unwrap(),
        )
        .unwrap();
        if profile {
            std::fs::write(
                output.join(format!("{name}-stages.json")),
                serde_json::to_vec(&stages).unwrap(),
            )
            .unwrap();
        }
    }
    group.finish();
}
criterion_group!(benches, compare);
criterion_main!(benches);
