pub use workload::clips::CASES as CLIP_CASES;

#[path = "backend_workload.rs"]
mod workload;
pub use workload::{CASES, Workload};

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
    fn submit(&mut self, workload: &mut Workload) -> tileink::NativeSubmission {
        if let Some((fonts, text)) = &mut workload.text {
            self.renderer
                .render_retained_with_text(&workload.scene, fonts, text)
        } else {
            self.renderer.render_retained(&workload.scene)
        }
        .unwrap()
    }
    pub fn render(&mut self, workload: &mut Workload) {
        self.submit(workload).wait().unwrap();
    }
    pub fn image(&mut self, workload: &mut Workload) -> tileink::Image {
        let submission = if let Some((fonts, text)) = &mut workload.text {
            self.renderer
                .render_retained_to_image_with_text(&workload.scene, fonts, text)
        } else {
            self.renderer.render_retained_to_image(&workload.scene)
        };
        submission.unwrap().readback().unwrap()
    }
    pub fn profile(&mut self, workload: &mut Workload) -> [u64; 2] {
        let start = std::time::Instant::now();
        let submission = self.submit(workload);
        let submitted = start.elapsed();
        submission.wait().unwrap();
        [
            submitted.as_nanos() as u64,
            (start.elapsed() - submitted).as_nanos() as u64,
        ]
    }
}

#[allow(dead_code)]
#[path = "backend_immediate.rs"]
mod immediate;

// Only retained matrix cases override the default mode.
#[allow(dead_code)]
impl Gpu {
    pub fn set_incremental_mode(&mut self, mode: tileink::IncrementalRenderMode) {
        let mut config = self.renderer.incremental_render_config();
        config.mode = mode;
        self.renderer.set_incremental_render_config(config);
    }
}

#[allow(dead_code)]
#[path = "backend_pipelined.rs"]
mod pipelined;
#[allow(unused_imports)]
pub use pipelined::Batch;
