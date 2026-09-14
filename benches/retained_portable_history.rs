#[path = "../examples/common/benchmark_gpu.rs"]
mod benchmark_gpu;

use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use tileink::{
    Canvas, Filter, IncrementalRenderMode, Radius, Region, RetainedNodeId, RetainedParent,
    RetainedScene, WgpuRenderer,
};

struct Scene {
    retained: RetainedScene,
    changing: RetainedNodeId,
    leaves: [Rc<Canvas>; 4],
    cursor: usize,
}

impl Scene {
    fn new(width: u32, height: u32) -> Self {
        let root = RetainedNodeId::for_owner(884_100);
        let stable = RetainedNodeId::for_owner(884_101);
        let changing = RetainedNodeId::for_owner(884_102);
        let mut background = Canvas::new(width, height, 1.0);
        background.push_rect(
            Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
            Radius::ZERO,
            Color::from_rgb8(40, 80, 120),
        );
        let filtered = Rect::new(
            f64::from(width) * 0.6,
            f64::from(height) * 0.5,
            f64::from(width) - 2.0,
            f64::from(height) - 2.0,
        );
        background.push_filter_layer(Filter::Invert(1.0), Region::rect(filtered, Radius::ZERO));
        background.push_rect(filtered, Radius::ZERO, Color::from_rgb8(0, 0, 255));
        background.pop_layer();
        let leaves = std::array::from_fn(|index| {
            let mut leaf = Canvas::new(width, height, 1.0);
            let x = 17.0 + index as f64 * 4.0;
            leaf.push_rect(
                Rect::new(x, 17.0, x + 23.0, 31.0),
                Radius::ZERO,
                Color::from_rgb8(40 + index as u8 * 50, 180, 70),
            );
            Rc::new(leaf)
        });
        let mut retained = RetainedScene::new(width, height, 1.0, root).unwrap();
        retained
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                stable,
                Rc::new(background),
                Affine::IDENTITY,
            )
            .insert_scene(
                RetainedParent::content(root),
                None,
                changing,
                leaves[0].clone(),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        Self {
            retained,
            changing,
            leaves,
            cursor: 0,
        }
    }

    fn advance(&mut self) {
        self.cursor = (self.cursor + 1) % self.leaves.len();
        self.retained
            .transaction()
            .replace_scene(self.changing, self.leaves[self.cursor].clone())
            .commit()
            .unwrap();
    }
}

fn retained_portable_history(c: &mut Criterion) {
    let api = std::env::var("TILEINK_BENCH_API").unwrap_or_else(|_| "vulkan".into());
    let (_, device, queue) =
        benchmark_gpu::device(&api, true, false, wgpu::MemoryHints::Performance);
    let mut group = c.benchmark_group(format!("{api}_retained_portable_history"));
    group.throughput(Throughput::Elements(4));
    for (width, height) in [(300, 201), (1601, 1001)] {
        let mut scene = Scene::new(width, height);
        let mut renderer = WgpuRenderer::new(&device, &queue, width, height, Color::TRANSPARENT);
        let mut full = WgpuRenderer::new(&device, &queue, width, height, Color::TRANSPARENT);
        let mut config = full.incremental_render_config();
        config.mode = IncrementalRenderMode::ForceFull;
        full.set_incremental_render_config(config);
        // Check all four phases before timing, using independent output histories.
        for _ in 0..8 {
            scene.advance();
            renderer.render_retained(&scene.retained);
            full.render_retained(&scene.retained);
            assert!(
                renderer.image().pixels == full.image().pixels,
                "{width}x{height} phase {} differs from ForceFull",
                scene.cursor
            );
        }
        assert!(!renderer.incremental_render_stats().full_redraw);
        group.bench_function(format!("partial-offscreen-{width}x{height}"), |b| {
            b.iter_custom(|iterations| {
                let start = Instant::now();
                for _ in 0..iterations {
                    // Every sample has equal weight for every mutation phase.
                    for _ in 0..scene.leaves.len() {
                        scene.advance();
                        renderer.render_retained(&scene.retained);
                        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    }
                }
                start.elapsed()
            });
        });
        // Recheck the measured output outside timing.
        full.render_retained(&scene.retained);
        assert!(
            renderer.image().pixels == full.image().pixels,
            "measured output must match ForceFull at {width}x{height}"
        );
        eprintln!(
            "{api} {width}x{height}: {:?}",
            renderer.incremental_render_stats()
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(4));
    targets = retained_portable_history
}
criterion_main!(benches);
