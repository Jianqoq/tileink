use super::WgpuExecutionAdapter;
use crate::{
    render::{
        backdrops::{BackdropAdapter, BackdropPass},
        damage_tiles::DamageTiles,
    },
    shared::layer::{filter::Filter, region::Region},
    wgpu::renderer::{Bounds, FilterCursors, RenderTargetId, WgpuTarget},
};

impl BackdropAdapter for WgpuExecutionAdapter<'_> {
    type WorkState = Option<DamageTiles>;

    fn try_direct_backdrop(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        region: &Region,
    ) -> bool {
        match filter {
            Filter::Blur {
                std_dev_x,
                std_dev_y,
                sampling,
            } => self.renderer.apply_downsampled_blur_rect_composite(
                self.commands,
                target,
                bounds,
                *std_dev_x,
                *std_dev_y,
                *sampling,
                region,
            ),
            Filter::RectLiquidGlass(glass) => {
                self.renderer.apply_downsampled_liquid_glass_rect_composite(
                    self.commands,
                    target,
                    bounds,
                    *glass,
                    region,
                )
            }
            _ => false,
        }
    }

    fn suspend_backdrop_work(&mut self) -> Self::WorkState {
        self.renderer.suspend_incremental_filter_work()
    }

    fn restore_backdrop_work(&mut self, state: Self::WorkState) {
        self.renderer.restore_incremental_filter_work(state);
    }

    fn filter_backdrop(
        &mut self,
        pass: BackdropPass<'_>,
        cursors: &mut FilterCursors,
    ) -> Result<(), ()> {
        let BackdropPass {
            source,
            target,
            bounds,
            partial_output,
            filter,
            region,
        } = pass;
        let ok = if let Some(output) = partial_output {
            match filter {
                Filter::Blur {
                    std_dev_x,
                    std_dev_y,
                    ..
                } => {
                    cursors.advance_filter(filter);
                    self.renderer.apply_blur_from_source_partial(
                        self.commands,
                        source,
                        target,
                        output,
                        bounds,
                        *std_dev_x,
                        *std_dev_y,
                    )
                }
                Filter::RectLiquidGlass(glass) => {
                    crate::wgpu::renderer::rect_liquid_glass_region(Some(region), bounds)
                        .is_some_and(|region| {
                            self.renderer.apply_liquid_glass_from_source_partial(
                                self.commands,
                                source,
                                target,
                                output,
                                bounds,
                                *glass,
                                region,
                            )
                        })
                }
                _ => unreachable!("only blur and liquid glass support partial backdrop updates"),
            }
        } else {
            match filter {
                Filter::Blur {
                    std_dev_x,
                    std_dev_y,
                    sampling,
                } => self.renderer.apply_blur_from_source(
                    self.commands,
                    source,
                    target,
                    bounds,
                    *std_dev_x,
                    *std_dev_y,
                    *sampling,
                ),
                _ => {
                    self.renderer
                        .copy_region_to_target(self.commands, source, target, bounds)
                        && self.renderer.apply_filter(
                            self.commands,
                            target,
                            bounds,
                            filter,
                            Some(region),
                            cursors,
                        )
                }
            }
        };
        ok.then_some(()).ok_or(())
    }

    fn composite_backdrop_rect(
        &mut self,
        target: RenderTargetId,
        source: RenderTargetId,
        bounds: Bounds,
        region: &Region,
    ) -> Result<(), ()> {
        self.renderer
            .composite_src_over_rect_mask_direct(self.commands, target, source, bounds, region)
            .then_some(())
            .ok_or(())
    }

    fn composite_cached_backdrop_rect(
        &mut self,
        target: RenderTargetId,
        source: &WgpuTarget,
        bounds: Bounds,
        region: &Region,
    ) -> Result<(), ()> {
        self.renderer
            .composite_cached_backdrop_rect(self.commands, target, source, bounds, region)
            .then_some(())
            .ok_or(())
    }
}
