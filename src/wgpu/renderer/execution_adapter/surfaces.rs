//! WGPU resource operations for the shared group executor.

use super::*;
use crate::render::retained::RetainedRenderState;

impl crate::render::surfaces::SurfaceAdapter for WgpuExecutionAdapter<'_> {
    type Surface = WgpuTarget;
    fn size(&self) -> (u32, u32) {
        self.renderer.size
    }
    fn origin(&self) -> (i32, i32) {
        self.renderer.surface_origin
    }
    fn retained(&self) -> &RetainedRenderState<WgpuTarget> {
        &self.renderer.retained
    }
    fn retained_mut(&mut self) -> &mut RetainedRenderState<WgpuTarget> {
        &mut self.renderer.retained
    }
    fn acquire_scratch(&mut self) -> Result<RenderTargetId, ()> {
        self.renderer.acquire_scratch().ok_or(())
    }
    fn install_scratch(&mut self, target: RenderTargetId, surface: WgpuTarget) {
        self.renderer.install_scratch_render_target(target, surface);
    }
    fn take_scratch(&mut self, target: RenderTargetId) -> Option<WgpuTarget> {
        self.renderer.take_scratch_target(target)
    }
    fn release_scratch(&mut self, target: RenderTargetId) {
        self.renderer.release_scratch(target);
    }
    fn clear_target(&mut self, target: RenderTargetId) -> Result<(), ()> {
        self.renderer.clear_render_target(self.commands, target, 0);
        Ok(())
    }
    fn clear_region(&mut self, target: RenderTargetId, bounds: Bounds) -> Result<(), ()> {
        self.renderer
            .clear_render_region(self.commands, target, bounds, 0);
        Ok(())
    }
    fn composite_cached(
        &mut self,
        target: RenderTargetId,
        source: &WgpuTarget,
        mask: Option<&WgpuTarget>,
        bounds: Bounds,
        stack: Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> Result<(), ()> {
        self.renderer
            .composite_cached_group(self.commands, target, source, mask, bounds, stack, blend)
            .then_some(())
            .ok_or(())
    }
    fn composite_targets(
        &mut self,
        target: RenderTargetId,
        source: RenderTargetId,
        mask: RenderTargetId,
        bounds: Bounds,
        stack: Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> Result<(), ()> {
        self.renderer
            .composite_group_targets(self.commands, target, source, mask, bounds, stack, blend)
            .then_some(())
            .ok_or(())
    }
}
