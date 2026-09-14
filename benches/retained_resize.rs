#[path = "../examples/common/resize.rs"]
mod resize;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use peniko::Color;
use std::time::{Duration, Instant};
use tileink::WgpuRenderer;

fn retained_resize(c: &mut Criterion) {
    let api = std::env::var("TILEINK_BENCH_API").unwrap_or_else(|_| "vulkan".into());
    let mut group = c.benchmark_group(format!("{api}_retained_resize"));
    group.throughput(Throughput::Elements(resize::CYCLE as u64));
    for portable in [false, true] {
        let (_, device, queue) =
            resize::device(&api, portable, false, wgpu::MemoryHints::Performance);
        let mut scene = resize::Scene::new();
        let mut renderer = WgpuRenderer::new(
            &device,
            &queue,
            scene.size.0,
            scene.size.1,
            Color::TRANSPARENT,
        );
        // Validate every size before timing and warm a complete grow/shrink cycle.
        let mut full = resize::full_renderer(&device, &queue);
        scene.verify_cycle(&mut renderer, &mut full);
        let pipelines = renderer.pipeline_compilation_epoch();
        group.bench_function(if portable { "portable" } else { "native" }, |b| {
            b.iter_custom(|iterations| {
                let start = Instant::now();
                for _ in 0..iterations {
                    // One Criterion iteration is a complete cycle, preserving size weights.
                    for _ in 0..resize::CYCLE {
                        scene.advance();
                        renderer.render_retained(&scene.retained);
                        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    }
                }
                start.elapsed()
            });
        });
        assert_eq!(
            renderer.pipeline_compilation_epoch(),
            pipelines,
            "steady-state resize compiled a pipeline"
        );
        assert!(renderer.incremental_render_stats().full_redraw);
        scene.verify(&mut renderer, &mut full);
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(4));
    targets = retained_resize
}
criterion_main!(benches);
