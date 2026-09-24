use super::{Gpu, Workload};

// Burst submission measures CPU/GPU overlap without adding a swapchain or VSync.
// Receipts remain alive until all frames in the burst have been submitted.
pub struct Batch {
    pending: Vec<tileink::NativeSubmission>,
}

impl Batch {
    pub fn new(capacity: usize) -> Self {
        Self {
            pending: Vec::with_capacity(capacity),
        }
    }

    pub fn render(&mut self, gpu: &mut Gpu, workload: &mut Workload, frames: usize) {
        for _ in 0..frames {
            workload.advance();
            self.pending.push(gpu.submit(workload));
        }
        for receipt in self.pending.drain(..) {
            receipt.wait().unwrap();
        }
    }
}
