use super::*;

/// Drives the real lazy filter factory without compiling unrelated fine pipelines.
#[doc(hidden)]
pub struct FilterCompilationBenchmark {
    device: ::wgpu::Device,
    filters: WgpuFilterPipeline,
    tracker: PipelineCompilationTracker,
}

impl FilterCompilationBenchmark {
    /// Prepare the clear pipeline used by an ordinary first frame. This belongs
    /// to Criterion setup; the timed operation is the first subsequent filter.
    pub fn new(device: &::wgpu::Device) -> Self {
        let tracker = PipelineCompilationTracker::default();
        let filters = WgpuFilterPipeline::new(device, None, &tracker)
            .expect("filter benchmark requires supported storage limits");
        filters.kernel(device, &filters.clear_region);
        Self {
            device: device.clone(),
            filters,
            tracker,
        }
    }

    /// Returns newly created pipelines; repeated calls must return zero.
    pub fn compile_filter(&self) -> u64 {
        let before = self.tracker.epoch();
        for kernel in [
            &self.filters.copy_region,
            &self.filters.blur_shared_region,
            &self.filters.composite_direct_region,
        ] {
            self.filters.kernel(&self.device, kernel);
        }
        self.tracker.epoch() - before
    }
}
