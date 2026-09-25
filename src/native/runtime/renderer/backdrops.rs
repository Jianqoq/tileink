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
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        region: &Region,
    ) -> Result<bool> {
        use super::filter_encoding::FilterEncoding;
        use crate::render::filter_program::FilterExecutor;
        let work = self
            .retained
            .active_tiles()
            .map(|tiles| tiles.list().to_vec());
        let mut encoding = FilterEncoding {
            execution: self,
            work,
            error: None,
        };
        // Use the shared direct schedule: materializing low-resolution glass first
        // changes interpolation/rounding and cannot preserve identical pixels.
        let mut executor = FilterExecutor::new(&mut encoding);
        let applied = match filter {
            Filter::Blur {
                std_dev_x,
                std_dev_y,
                sampling,
            } => executor.apply_downsampled_blur_rect_composite(
                target, bounds, *std_dev_x, *std_dev_y, *sampling, region,
            ),
            Filter::RectLiquidGlass(glass) => executor
                .apply_downsampled_liquid_glass_rect_composite(target, bounds, *glass, region),
            _ => false,
        };
        match encoding.error {
            Some(error) => Err(error),
            None => Ok(applied),
        }
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
        if let Some(output) = pass.partial_output {
            let work = self
                .retained
                .active_tiles()
                .map(|tiles| tiles.list().to_vec());
            let mut encoding = super::filter_encoding::FilterEncoding {
                execution: self,
                work,
                error: None,
            };
            let mut executor = crate::render::filter_program::FilterExecutor::new(&mut encoding);
            let success = match pass.filter {
                Filter::Blur {
                    std_dev_x,
                    std_dev_y,
                    ..
                } => {
                    cursors.advance_filter(pass.filter);
                    executor.apply_blur_from_source_partial(
                        pass.source,
                        pass.target,
                        output,
                        pass.bounds,
                        *std_dev_x,
                        *std_dev_y,
                    )
                }
                Filter::RectLiquidGlass(glass) if matches!(pass.region, Region::Rect { .. }) => {
                    executor.apply_liquid_glass_from_source_partial(
                        pass.source,
                        pass.target,
                        output,
                        pass.bounds,
                        *glass,
                        crate::shared::layer::filter::rect_liquid_glass_region(
                            Some(pass.region),
                            pass.bounds,
                        ),
                    )
                }
                _ => return Err("unsupported native partial backdrop operation".into()),
            };
            return match encoding.error {
                Some(error) => Err(error),
                None if success => Ok(()),
                None => Err("native partial backdrop recording failed".into()),
            };
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
        let source = source.image_in(self.batch)?;
        self.composite_backdrop_image(target, source, bounds, region)
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
            self.retained
                .active_tiles()
                .map(crate::render::damage_tiles::DamageTiles::list),
            self.targets.get(target)?.image(),
        )
    }
}
