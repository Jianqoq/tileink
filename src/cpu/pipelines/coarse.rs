impl CoarseGpuPrepared {
    pub fn run(&self) {}
}

pub struct CoarseGpuPrepared {}

pub struct CoarseGpuPipeline;

impl CoarseGpuPipeline {
    pub fn new() -> Self {
        Self
    }

    pub fn prepare(&self) -> CoarseGpuPrepared {
        CoarseGpuPrepared {}
    }
}
