//! Native adapter for the shared Result-based filter scheduler.
use super::*;
use crate::render::{
    damage_tiles::DamageTiles,
    filter_program::{FilterAdapter, FilterExecutor, FilterKernel},
};
use crate::shared::layer::{filter::Filter, region::Region};

pub(super) struct FilterEncoding<'a, 'b> {
    pub(super) execution: &'a mut Execution<'b>,
    pub(super) work: Option<Vec<u32>>,
}
impl Execution<'_> {
    pub(super) fn apply_filter(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        region: Option<&Region>,
        cursors: &mut FilterCursors,
    ) -> Result<()> {
        let work = self
            .retained
            .active_tiles()
            .map(|tiles| tiles.list().to_vec());
        let mut encoding = FilterEncoding {
            execution: self,
            work,
        };
        FilterExecutor::new(&mut encoding)
            .apply_filter(target, bounds, filter, region, false, cursors)
    }
}
impl FilterAdapter for FilterEncoding<'_, '_> {
    type Error = Box<dyn std::error::Error>;
    type Work = Vec<u32>;
    fn size(&self) -> (u32, u32) {
        self.execution.targets.size()
    }
    fn acquire_scratch(&mut self) -> Result<RenderTargetId> {
        self.execution.targets.acquire(self.execution.batch)
    }
    fn release_scratch(&mut self, target: RenderTargetId) -> Result<()> {
        self.execution.targets.release(target)
    }
    fn encode(&mut self, kernel: FilterKernel<'_>) -> Result<()> {
        self.record(kernel)
    }
    fn active_tiles(&self) -> Option<&DamageTiles> {
        self.execution.retained.active_tiles()
    }
    fn filter_work(&self) -> Option<Self::Work> {
        self.work.clone()
    }
    fn set_filter_work(&mut self, work: Option<Self::Work>) {
        self.work = work;
    }
    fn prepare_filter_tile_work(&mut self, tiles: &[u32]) {
        self.work = Some(tiles.to_vec());
    }
    fn suspend_incremental_filter_work(&mut self) -> Option<DamageTiles> {
        let previous = self.execution.retained.active_tiles().cloned();
        self.execution.retained.set_active_tiles(None);
        self.work = None;
        previous
    }
    fn restore_incremental_filter_work(&mut self, active: Option<DamageTiles>) {
        self.work = active.as_ref().map(|tiles| tiles.list().to_vec());
        self.execution.retained.set_active_tiles(active);
    }
}
