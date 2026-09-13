use crate::render::damage_tiles::DamageTiles;
use crate::render::filter_program::{ColorFilterKind, FilterAdapter, FilterExecutor, FilterKernel};
use peniko::Mix;

use crate::render::filter_resources::cursors::FilterCursors;
use crate::shared::{
    bounds::Bounds,
    layer::filter::{self as filter_model, Filter},
};

use super::super::{commands::WgpuCommandBatch, filter_work::FilterTileWork};
use super::{RenderTargetId, Renderer, encode_morphology_operator};

mod encoder;
mod kernels;
use encoder::WgpuFilterEncoder;
impl Renderer {
    pub(super) fn apply_filter(
        &mut self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        region: Option<&crate::shared::layer::region::Region>,
        filter_cursors: &mut FilterCursors,
    ) -> bool {
        FilterExecutor::new(&mut WgpuFilterEncoder {
            renderer: self,
            commands,
        })
        .apply_filter(target, bounds, filter, region, filter_cursors)
    }
    pub(super) fn apply_blur_from_source(
        &mut self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
    ) -> bool {
        FilterExecutor::new(&mut WgpuFilterEncoder {
            renderer: self,
            commands,
        })
        .apply_blur_from_source(source, target, bounds, std_dev_x, std_dev_y, sampling)
    }
    /// Updates only `output_bounds` of an existing blur target while sampling
    /// the complete source texture. The horizontal intermediate is expanded
    /// vertically so the second pass never observes an uninitialized row.
    pub(super) fn apply_blur_from_source_partial(
        &mut self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        output_bounds: Bounds,
        sample_bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
    ) -> bool {
        FilterExecutor::new(&mut WgpuFilterEncoder {
            renderer: self,
            commands,
        })
        .apply_blur_from_source_partial(
            source,
            target,
            output_bounds,
            sample_bounds,
            std_dev_x,
            std_dev_y,
        )
    }
    pub(super) fn apply_downsampled_blur_rect_composite(
        &mut self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
        region: &crate::shared::layer::region::Region,
    ) -> bool {
        FilterExecutor::new(&mut WgpuFilterEncoder {
            renderer: self,
            commands,
        })
        .apply_downsampled_blur_rect_composite(
            target, bounds, std_dev_x, std_dev_y, sampling, region,
        )
    }
    pub(super) fn apply_downsampled_liquid_glass_rect_composite(
        &mut self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: &crate::shared::layer::region::Region,
    ) -> bool {
        FilterExecutor::new(&mut WgpuFilterEncoder {
            renderer: self,
            commands,
        })
        .apply_downsampled_liquid_glass_rect_composite(target, bounds, glass, region)
    }
    /// Recomputes only the damaged output of a cached full-resolution liquid-glass backdrop.
    ///
    /// `source` is the complete painter-order history captured before the backdrop. The blurred
    /// temporary needs a rectangular refraction halo beyond the compact root worklist, so that
    /// halo is generated with compact dispatch temporarily suspended; the final glass pass then
    /// returns to the exact dirty-tile worklist and leaves clean cached output untouched.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_liquid_glass_from_source_partial(
        &mut self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        output_bounds: Bounds,
        sample_bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> bool {
        FilterExecutor::new(&mut WgpuFilterEncoder {
            renderer: self,
            commands,
        })
        .apply_liquid_glass_from_source_partial(
            source,
            target,
            output_bounds,
            sample_bounds,
            glass,
            region,
        )
    }
}
