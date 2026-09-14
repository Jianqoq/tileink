//! Shared filter control flow. GPU adapters encode one typed kernel at a time.
//! Scratch lifetimes, graph dependencies and multi-pass ordering belong here so
//! future native adapters cannot silently implement different filter semantics.
use crate::{
    render::{
        damage_tiles::DamageTiles, filter_resources::cursors::FilterCursors, output::RenderTargetId,
    },
    shared::{
        bounds::Bounds,
        layer::filter::{self as filter_model, Filter},
    },
};
use peniko::Mix;
mod kernels;
pub(crate) use kernels::{ColorFilterKind, FilterKernel};
mod blur;
mod effects;
mod glass;
mod graph;

pub(crate) trait FilterAdapter {
    type Work;
    fn size(&self) -> (u32, u32);
    fn acquire_scratch(&mut self) -> Option<RenderTargetId>;
    fn release_scratch(&mut self, target: RenderTargetId);
    fn encode(&mut self, kernel: FilterKernel<'_>) -> bool;
    fn active_tiles(&self) -> Option<&DamageTiles>;
    fn filter_work(&self) -> Option<Self::Work>;
    fn set_filter_work(&mut self, work: Option<Self::Work>);
    fn prepare_filter_tile_work(&mut self, tiles: &[u32]);
    fn suspend_incremental_filter_work(&mut self) -> Option<DamageTiles>;
    fn restore_incremental_filter_work(&mut self, active: Option<DamageTiles>);
}

pub(crate) struct FilterExecutor<'a, A: FilterAdapter> {
    adapter: &'a mut A,
}
impl<'a, A: FilterAdapter> FilterExecutor<'a, A> {
    pub(crate) fn new(adapter: &'a mut A) -> Self {
        Self { adapter }
    }
}

pub(crate) fn rect_liquid_glass_region(
    region: Option<&crate::shared::layer::region::Region>,
    fallback_bounds: Bounds,
) -> Option<filter_model::RectLiquidGlassRegion> {
    match region {
        Some(crate::shared::layer::region::Region::Rect { .. }) | None => Some(
            filter_model::rect_liquid_glass_region(region, fallback_bounds),
        ),
        Some(crate::shared::layer::region::Region::Path { .. }) => None,
    }
}
fn should_sample_downsampled_blur_in_liquid_glass(glass: filter_model::RectLiquidGlass) -> bool {
    glass.refraction_dispersion.abs() <= f32::EPSILON
        && glass.fresnel_factor <= 0.0
        && glass.glare_factor <= 0.0
}
fn downsampled_bounds(bounds: Bounds, downsample: u32) -> Option<Bounds> {
    let downsample = downsample.max(1) as i32;
    let bounds = Bounds::new(
        bounds.x0.div_euclid(downsample),
        bounds.y0.div_euclid(downsample),
        div_ceil_i32(bounds.x1, downsample),
        div_ceil_i32(bounds.y1, downsample),
    );
    (!bounds.is_empty()).then_some(bounds)
}
fn div_ceil_i32(value: i32, divisor: i32) -> i32 {
    -((-value).div_euclid(divisor))
}
#[cfg(test)]
mod tests;
