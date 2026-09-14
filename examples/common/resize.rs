//! Fixed retained geometry with changing output dimensions for resize measurements.
//! Geometry and pipelines are reused; target/history allocation remains part of rendering.

use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;
use tileink::{
    Canvas, Filter, IncrementalRenderMode, Radius, Region, RetainedNodeId, RetainedParent,
    RetainedScene, WgpuRenderer,
};

pub const CYCLE: usize = 64;

#[path = "benchmark_gpu.rs"]
mod benchmark_gpu;
pub use benchmark_gpu::device;

pub struct Scene {
    pub retained: RetainedScene,
    pub size: (u32, u32),
    cursor: usize,
}

impl Scene {
    pub fn new() -> Self {
        let root = RetainedNodeId::for_owner(884_200);
        let mut canvas = Canvas::new(1217, 761, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 1217.0, 761.0),
            Radius::ZERO,
            Color::from_rgb8(24, 31, 43),
        );
        for row in 0..24 {
            for column in 0..16 {
                let x = 12.25 + f64::from(column) * 75.0;
                let y = 12.5 + f64::from(row) * 31.0;
                canvas.push_rect(
                    Rect::new(x, y, x + 66.5, y + 23.25),
                    Radius::ZERO,
                    Color::from_rgba8(40 + column * 10, 40 + row * 7, 160, 211),
                );
            }
        }
        let filtered = Rect::new(80.0, 96.0, 560.0, 420.0);
        canvas.push_filter_layer(
            Filter::Blur {
                std_dev_x: 2.25,
                std_dev_y: 1.75,
                sampling: Default::default(),
            },
            Region::rect(filtered, Radius::ZERO),
        );
        canvas.push_rect(filtered, Radius::ZERO, Color::from_rgba8(210, 80, 40, 101));
        canvas.pop_layer();
        let size = (961, 601);
        let mut retained = RetainedScene::new(size.0, size.1, 1.0, root).unwrap();
        retained
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(884_201),
                Rc::new(canvas),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        Self {
            retained,
            size,
            cursor: 0,
        }
    }

    pub fn advance(&mut self) {
        self.cursor = (self.cursor + 1) % CYCLE;
        let step = self.cursor.min(CYCLE - self.cursor) as u32;
        self.size = (961 + step * 8, 601 + step * 5);
        self.retained
            .transaction()
            .resize(self.size.0, self.size.1, 1.0)
            .commit()
            .unwrap();
    }

    pub fn verify(&self, renderer: &mut WgpuRenderer, full: &mut WgpuRenderer) {
        let actual = renderer.image();
        assert_eq!((actual.width, actual.height), self.size);
        full.render_retained(&self.retained);
        assert!(
            actual.pixels == full.image().pixels,
            "resize history differs from ForceFull at {:?}",
            self.size
        );
        assert!(actual.pixels.iter().any(|pixel| *pixel != 0));
    }

    pub fn verify_cycle(&mut self, renderer: &mut WgpuRenderer, full: &mut WgpuRenderer) {
        // Seed actual phase-zero history before checking the grow/shrink transitions.
        renderer.render_retained(&self.retained);
        self.verify(renderer, full);
        for _ in 0..CYCLE {
            self.advance();
            renderer.render_retained(&self.retained);
            self.verify(renderer, full);
        }
    }
}

pub fn full_renderer(device: &wgpu::Device, queue: &wgpu::Queue) -> WgpuRenderer {
    let mut renderer = WgpuRenderer::new(device, queue, 1, 1, Color::TRANSPARENT);
    let mut config = renderer.incremental_render_config();
    config.mode = IncrementalRenderMode::ForceFull;
    renderer.set_incremental_render_config(config);
    renderer
}
