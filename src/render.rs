use crate::scene::Scene;

pub trait Render {
    type ScanPrepared;
    type CumsumPrepared;
    type BinPrepared;
    type CoarsePrepared;
    type FinePrepared;
    type ExecuteArgs<'a>;
    fn render(&mut self, scene: &Scene);
    fn execute(&self, scene: &Scene, args: Self::ExecuteArgs<'_>);
    fn prepare_scan(&self);
    fn prepare_cumsum(&self);
    fn prepare_bin(&self);
    fn prepare_coarse(&self);
    fn prepare_fine(&self);
    fn flush(
        &self,
        scan: Self::ScanPrepared,
        cumsum: Self::CumsumPrepared,
        bin: Self::BinPrepared,
        coarse: Self::CoarsePrepared,
        fine: Self::FinePrepared,
    );
}
