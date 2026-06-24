use crate::{cpu::computes::cumsum::run_backdrop_cumsum, shared::bd_record::BackdropRecord};

pub struct CumsumCpuPipeline {}

pub struct CumsumPrepared<'a> {
    backdrops: &'a mut Vec<i32>,
    backdrop_records: &'a [BackdropRecord],
}

impl<'a> CumsumPrepared<'a> {
    pub fn run(&mut self) {
        run_backdrop_cumsum(self.backdrops.as_mut_slice(), self.backdrop_records);
    }
}

impl CumsumCpuPipeline {
    pub fn new() -> Self {
        Self {}
    }

    pub fn prepare<'a>(
        &self,
        backdrops: &'a mut Vec<i32>,
        backdrop_records: &'a [BackdropRecord],
    ) -> CumsumPrepared<'a> {
        CumsumPrepared {
            backdrops,
            backdrop_records,
        }
    }
}
