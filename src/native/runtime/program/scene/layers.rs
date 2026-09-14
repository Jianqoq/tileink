//! Filter consumers share the scene-owned scan and layer buffers.
use super::Scene;
use crate::native::runtime::program::filter::{layer::Geometry, stack::Stack};

impl Scene {
    pub(crate) fn filter_geometry(&self) -> Geometry {
        // SAFETY: SceneCache recorded these bounded Canvas records and scan in
        // one batch. Filter recording checks resource ownership before use.
        unsafe {
            Geometry::from_scan(
                self.draws,
                &self.scan,
                self.paint,
                self.draw_count,
                self.fine_params.paint_sdf_shadow_base,
            )
        }
    }

    pub(crate) fn filter_stack(&self) -> Stack {
        // SAFETY: shared LayerStackRecord conversion and plan preparation bound
        // draw indices and quantize opacity. Upload checked its byte capacity.
        unsafe { Stack::from_gpu(self.filter_geometry(), self.layers, self.layer_count) }
    }
}
