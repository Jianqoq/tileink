//! Filter consumers share the scene-owned scan and layer buffers.
use super::Scene;
use crate::native::runtime::program::filter::{layer::Geometry, stack::ClipStack};

impl Scene {
    pub(crate) const fn filter_geometry(&self) -> Geometry {
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

    pub(crate) const fn filter_clip_stack(&self) -> ClipStack {
        // SAFETY: shared LayerStackRecord conversion and plan preparation bound
        // draw indices. Offscreen ranges contain only ancestor clips; fused groups
        // remain in draw-batch ranges. Upload checked the full buffer capacity.
        unsafe { ClipStack::from_gpu(self.filter_geometry(), self.layers, self.layer_count) }
    }
}
