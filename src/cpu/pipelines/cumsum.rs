use crate::{cpu::computes::cumsum::run_backdrop_cumsum, shared::path::PathRecord};

pub struct CumsumCpuPipeline {}

pub struct CumsumPrepared<'a> {
    backdrops: &'a mut Vec<i32>,
    path_records: &'a [PathRecord],
}

impl<'a> CumsumPrepared<'a> {
    pub fn run(&mut self) {
        run_backdrop_cumsum(self.backdrops.as_mut_slice(), self.path_records);
    }
}

impl CumsumCpuPipeline {
    pub fn new() -> Self {
        Self {}
    }

    pub fn prepare<'a>(
        &self,
        backdrops: &'a mut Vec<i32>,
        path_records: &'a [PathRecord],
    ) -> CumsumPrepared<'a> {
        CumsumPrepared {
            backdrops,
            path_records,
        }
    }
}
