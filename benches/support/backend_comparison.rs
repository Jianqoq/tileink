use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;
use tileink::{Canvas, Radius, RetainedNodeId, RetainedParent, RetainedScene};

#[cfg(feature = "wgpu")]
#[path = "../../examples/common/benchmark_gpu.rs"]
mod benchmark_gpu;

pub struct Workload {
    pub scene: RetainedScene,
    name: &'static str,
    phase: u32,
}

impl Workload {
    pub fn new(name: &'static str) -> Self {
        let root = RetainedNodeId::for_owner(1);
        let mut scene = RetainedScene::new(1280, 800, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        for index in 0..384 {
            let mut canvas = Canvas::new(1280, 800, 1.0);
            let rect = Rect::new(0.25, 0.5, 65.75, 23.25);
            if name == "blur" && index < 8 {
                canvas.push_filter_layer(
                    tileink::Filter::Blur {
                        std_dev_x: 2.0,
                        std_dev_y: 2.0,
                        sampling: Default::default(),
                    },
                    tileink::Region::rect(Rect::new(0.0, 0.0, 72.0, 30.0), Radius::ZERO),
                );
            }
            canvas.push_rect(
                rect,
                Radius::ZERO,
                Color::from_rgba8((40 + index % 160) as u8, 110, 190, 211),
            );
            if name == "blur" && index < 8 {
                canvas.pop_layer();
            }
            transaction.insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(index + 2),
                Rc::new(canvas),
                Affine::translate((
                    (index % 16) as f64 * 76.0 + 8.0,
                    (index / 16) as f64 * 32.0 + 8.0,
                )),
            );
        }
        transaction.commit().unwrap();
        Self {
            scene,
            name,
            phase: 0,
        }
    }

    pub fn size(&self) -> [u32; 2] {
        let step = if self.name == "resize" {
            self.phase.min(16 - self.phase)
        } else {
            0
        };
        [1280 - step * 8, 800 - step * 5]
    }

    pub fn advance(&mut self) {
        self.phase = (self.phase + 1) % 16;
        if self.name == "unchanged" {
            return;
        }
        let mut transaction = self.scene.transaction();
        if self.name == "resize" {
            let step = self.phase.min(16 - self.phase);
            transaction.resize(1280 - step * 8, 800 - step * 5, 1.0);
        } else {
            let count = if self.name == "sparse" { 1 } else { 384 };
            for index in 0..count {
                transaction.set_transform(
                    RetainedNodeId::for_owner(index + 2),
                    Affine::translate((
                        (index % 16) as f64 * 76.0 + 8.0 + f64::from(self.phase % 2),
                        (index / 16) as f64 * 32.0 + 8.0,
                    )),
                );
            }
        }
        transaction.commit().unwrap();
    }
}

#[cfg(feature = "wgpu")]
pub struct Gpu {
    renderer: tileink::WgpuRenderer,
    device: wgpu::Device,
}

#[cfg(feature = "wgpu")]
impl Gpu {
    pub fn new() -> Self {
        let api = std::env::var("TILEINK_BENCH_API").expect("set explicit wgpu API");
        let (_, device, queue) =
            benchmark_gpu::device(&api, false, false, wgpu::MemoryHints::Performance);
        Self {
            renderer: tileink::WgpuRenderer::new(&device, &queue, 1280, 800, Color::TRANSPARENT),
            device,
        }
    }
    pub fn render(&mut self, scene: &RetainedScene) {
        self.renderer.render_retained(scene);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
    }
    pub fn image(&mut self, _: &RetainedScene) -> tileink::Image {
        self.renderer.image()
    }
    pub fn profile(&mut self, scene: &RetainedScene) -> [u64; 2] {
        let start = std::time::Instant::now();
        self.renderer.render_retained(scene);
        let submitted = start.elapsed();
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        [
            submitted.as_nanos() as u64,
            (start.elapsed() - submitted).as_nanos() as u64,
        ]
    }
}

#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
pub struct Gpu {
    renderer: tileink::NativeRenderer,
}

#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
impl Gpu {
    pub fn new() -> Self {
        #[cfg(feature = "dx12")]
        let api = tileink::NativeBackend::Dx12;
        #[cfg(feature = "vulkan")]
        let api = tileink::NativeBackend::Vulkan;
        #[cfg(feature = "metal")]
        let api = tileink::NativeBackend::Metal;
        let context = tileink::NativeContext::new(
            api,
            &tileink::NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_BENCH_GPU").expect("pin the GPU")),
                validation: false,
            },
        )
        .unwrap();
        Self {
            renderer: tileink::NativeRenderer::with_context(&context, 1280, 800).unwrap(),
        }
    }
    pub fn render(&mut self, scene: &RetainedScene) {
        self.renderer
            .render_retained(scene)
            .unwrap()
            .wait()
            .unwrap();
    }
    pub fn image(&mut self, scene: &RetainedScene) -> tileink::Image {
        self.renderer
            .render_retained_to_image(scene)
            .unwrap()
            .readback()
            .unwrap()
    }
    pub fn profile(&mut self, scene: &RetainedScene) -> [u64; 2] {
        let start = std::time::Instant::now();
        let submission = self.renderer.render_retained(scene).unwrap();
        let submitted = start.elapsed();
        submission.wait().unwrap();
        [
            submitted.as_nanos() as u64,
            (start.elapsed() - submitted).as_nanos() as u64,
        ]
    }
}
