//! Replay-size CPU staging workload: compare the former concatenation with direct mapped writes.
#[path = "../src/native/runtime/vulkan/upload.rs"]
mod upload;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::{hint::black_box, time::Duration};

fn benchmark(c: &mut Criterion) {
    let reference = std::env::var_os("TILEINK_BENCH_CONCATENATED_UPLOAD").is_some();
    let uniform = vec![17; 4096];
    let resources = vec![vec![29; 512 * 1024]; 32];
    let grids = [[3u8; 16]; 32];
    let mut destination = vec![0u8; 17 * 1024 * 1024];
    let mut group = c.benchmark_group("vulkan_frame_upload");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    group.throughput(Throughput::Bytes(16 * 1024 * 1024));
    group.bench_function("replay_16_mib", |b| {
        b.iter(|| {
            let uniforms = uniform.clone();
            if reference {
                let mut bytes = uniforms;
                for resource in &resources {
                    bytes.extend_from_slice(black_box(resource));
                }
                for grid in &grids {
                    bytes.resize((bytes.len() + 255) & !255, 0);
                    bytes.extend_from_slice(grid);
                }
                destination[..bytes.len()].copy_from_slice(&bytes);
            } else {
                let mut plan = upload::Upload::new();
                plan.push(&uniforms, 1).unwrap();
                for resource in &resources {
                    plan.push(black_box(resource), 1).unwrap();
                }
                for grid in &grids {
                    plan.push(grid, 256).unwrap();
                }
                assert!(plan.len() <= destination.len());
                unsafe {
                    plan.copy_to(destination.as_mut_ptr());
                }
            }
            black_box(&destination);
        })
    });
    group.finish();
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
