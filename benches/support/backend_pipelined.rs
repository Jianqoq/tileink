use super::{Gpu, Workload};

// Burst submission measures CPU/GPU overlap without adding a swapchain or VSync.
// Receipts remain alive until all frames in the burst have been submitted.
pub struct Batch {
    #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
    pending: Vec<tileink::NativeSubmission>,
}

impl Batch {
    pub fn new(capacity: usize) -> Self {
        #[cfg(feature = "wgpu")]
        let _ = capacity;
        Self {
            #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
            pending: Vec::with_capacity(capacity),
        }
    }

    pub fn render(&mut self, gpu: &mut Gpu, workload: &mut Workload, frames: usize) {
        for _ in 0..frames {
            workload.advance();
            #[cfg(feature = "wgpu")]
            {
                if let Some((fonts, text)) = &mut workload.text {
                    gpu.renderer
                        .render_retained_with_text(&workload.scene, fonts, text);
                } else {
                    gpu.renderer.render_retained(&workload.scene);
                }
            }
            #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
            self.pending.push(gpu.submit(workload));
        }
        #[cfg(feature = "wgpu")]
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
        for receipt in self.pending.drain(..) {
            receipt.wait().unwrap();
        }
    }
}
