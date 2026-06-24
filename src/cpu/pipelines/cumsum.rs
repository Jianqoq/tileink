pub struct CumsumCpuPipeline {}

pub struct CumsumPrepared {}

impl CumsumPrepared {
    pub fn run(&self) {}
}

impl CumsumCpuPipeline {
    pub fn new() -> Self {
        Self {}
    }

    pub fn prepare(&self, backdrops: &mut Vec<i32>) -> CumsumPrepared {
        CumsumPrepared {}
    }
}
