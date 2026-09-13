use super::WgpuExecutionAdapter;
use crate::render::{masks::MaskAdapter, output::RenderTargetId};
use crate::shared::{
    bounds::Bounds,
    layer::{mask::MaskKind, region::Region},
};

impl MaskAdapter for WgpuExecutionAdapter<'_> {
    fn mask_coverage(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        kind: MaskKind,
    ) -> Result<(), ()> {
        self.renderer
            .svg_mask_coverage(self.commands, source, target, bounds, kind);
        Ok(())
    }
    fn region_mask(
        &mut self,
        target: RenderTargetId,
        region: &Region,
        path: Option<u32>,
        bounds: Bounds,
    ) -> Result<(), ()> {
        self.renderer
            .build_region_mask(self.commands, target, region, path, bounds)
            .then_some(())
            .ok_or(())
    }
    fn apply_region(
        &mut self,
        mask: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> Result<(), ()> {
        self.renderer
            .apply_region_mask(self.commands, mask, target, bounds);
        Ok(())
    }
}
