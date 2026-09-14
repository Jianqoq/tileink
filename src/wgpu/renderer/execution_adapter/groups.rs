//! WGPU resource operations for the shared group executor.

use super::*;
use crate::render::groups::GroupAdapter;

impl GroupAdapter for WgpuExecutionAdapter<'_> {
    fn apply_opacity(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        opacity: f32,
    ) -> Result<(), ()> {
        // Root cause: opacity groups previously reported success when their
        // pointwise kernel could not obtain its pipeline or source snapshot.
        self.renderer
            .apply_color_filter_to_target(self.commands, target, bounds, FILTER_OPACITY, opacity)
            .then_some(())
            .ok_or(())
    }
    fn build_mask(&mut self, target: RenderTargetId, draw: u32, bounds: Bounds) -> Result<(), ()> {
        self.renderer
            .build_layer_mask(self.commands, target, draw, bounds);
        Ok(())
    }
}
