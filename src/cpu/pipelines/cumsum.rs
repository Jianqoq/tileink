pub struct BackdropCumsumGpuPipeline {}

pub struct BackdropCumsumPrepared {}

impl BackdropCumsumPrepared {
    pub fn run(&self) {}
}

impl BackdropCumsumGpuPipeline {
    pub fn new() -> Self {
        Self {}
    }

    pub fn prepare(&self) -> BackdropCumsumPrepared {
        BackdropCumsumPrepared {}
    }
}
