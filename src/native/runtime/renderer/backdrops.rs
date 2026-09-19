use super::*;
use crate::native::runtime::program::filter::rectangle::{self, RectanglePass};
use crate::render::{
    backdrops::{BackdropAdapter, BackdropPass},
    damage_tiles::DamageTiles,
    filter_pass::FilterPassAdapter,
};
use crate::shared::layer::{filter::Filter, region::Region};

impl BackdropAdapter for Execution<'_> {
    type WorkState = Option<DamageTiles>;
    fn try_direct_backdrop(
        &mut self,
        _target: RenderTargetId,
        _bounds: Bounds,
        _filter: &Filter,
        _region: &Region,
    ) -> bool {
        false
    }
    fn suspend_backdrop_work(&mut self) -> Self::WorkState {
        let previous = self.retained.active_tiles().cloned();
        self.retained.set_active_tiles(None);
        previous
    }
    fn restore_backdrop_work(&mut self, state: Self::WorkState) {
        self.retained.set_active_tiles(state);
    }
    fn filter_backdrop(
        &mut self,
        pass: BackdropPass<'_>,
        cursors: &mut FilterCursors,
    ) -> Result<()> {
        if pass.partial_output.is_some() {
            return Err("native partial backdrop history requires retained frame support".into());
        }
        self.copy_filter_region(pass.source, pass.target, pass.bounds)?;
        self.apply_filter(
            pass.target,
            pass.bounds,
            pass.filter,
            Some(pass.region),
            cursors,
        )
    }
    fn composite_backdrop_rect(
        &mut self,
        target: RenderTargetId,
        source: RenderTargetId,
        bounds: Bounds,
        region: &Region,
    ) -> Result<()> {
        self.composite_backdrop_image(target, self.targets.get(source)?.image(), bounds, region)
    }
    fn composite_cached_backdrop_rect(
        &mut self,
        target: RenderTargetId,
        source: &Surface,
        bounds: Bounds,
        region: &Region,
    ) -> Result<()> {
        self.composite_backdrop_image(target, source.image(), bounds, region)
    }
}
impl Execution<'_> {
    fn composite_backdrop_image(
        &mut self,
        target: RenderTargetId,
        source: ResourceId,
        bounds: Bounds,
        region: &Region,
    ) -> Result<()> {
        let Some(mut c) = self.config(bounds) else {
            return Ok(());
        };
        filter_kernels::configure_region(&mut c, region)?;
        rectangle::encode(
            self.batch,
            RectanglePass::Rectangle { source },
            c,
            None,
            self.targets.get(target)?.image(),
        )
    }
}
