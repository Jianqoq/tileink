//! Opaque offscreen allocation and composition operations shared by layer algorithms.
use crate::render::output::RenderTargetId;
use crate::render::{
    operations::OperationAdapter, retained::RetainedRenderState,
    retained_surfaces::SurfaceAllocation,
};
use crate::shared::bounds::Bounds;
use std::ops::Range;

/// Logical scratch occupancy ends after recording. Adapters must separately pin
/// every resource referenced by submitted work until that work completes.
pub(crate) trait SurfaceAdapter: OperationAdapter {
    type Surface: SurfaceAllocation;
    fn size(&self) -> (u32, u32);
    fn origin(&self) -> (i32, i32);
    fn retained(&self) -> &RetainedRenderState<Self::Surface>;
    fn retained_mut(&mut self) -> &mut RetainedRenderState<Self::Surface>;
    fn acquire_scratch(&mut self) -> Result<RenderTargetId, Self::Error>;
    fn install_scratch(&mut self, target: RenderTargetId, surface: Self::Surface);
    fn take_scratch(&mut self, target: RenderTargetId) -> Option<Self::Surface>;
    fn release_scratch(&mut self, target: RenderTargetId);
    fn clear_target(&mut self, target: RenderTargetId) -> Result<(), Self::Error>;
    fn clear_region(&mut self, target: RenderTargetId, bounds: Bounds) -> Result<(), Self::Error>;
    fn composite_cached(
        &mut self,
        target: RenderTargetId,
        source: &Self::Surface,
        mask: Option<&Self::Surface>,
        bounds: Bounds,
        stack: Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> Result<(), Self::Error>;
    fn composite_targets(
        &mut self,
        target: RenderTargetId,
        source: RenderTargetId,
        mask: RenderTargetId,
        bounds: Bounds,
        stack: Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> Result<(), Self::Error>;
}

#[cfg(test)]
pub(crate) mod test_support;
