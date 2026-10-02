use super::*;
use crate::native::runtime::program::filter::{
    self, BasicFilter,
    inputs::{self, InputFilter},
    rectangle::{self, RectanglePass},
};
use crate::{MaskKind, Region, render::masks::MaskAdapter};

impl MaskAdapter for Execution<'_> {
    fn mask_coverage(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        kind: MaskKind,
    ) -> Result<()> {
        let Some(mut config) = self.config(bounds) else {
            return Ok(());
        };
        config.mask_kind = match kind {
            MaskKind::Alpha => 0,
            MaskKind::Luminance => crate::shared::gpu_constants::SVG_MASK_LUMINANCE,
        };
        filter::encode(
            self.batch,
            BasicFilter::SvgMask,
            config,
            self.retained
                .active_tiles()
                .map(crate::render::damage_tiles::DamageTiles::list),
            Some(self.targets.get(source)?.image()),
            self.targets.get(target)?.image(),
        )
    }
    fn region_mask(
        &mut self,
        target: RenderTargetId,
        region: &Region,
        path: Option<u32>,
        bounds: Bounds,
    ) -> Result<()> {
        let Some(mut config) = self.config(bounds) else {
            return Ok(());
        };
        let target = self.targets.get(target)?.image();
        match region {
            Region::Rect { rect, radius } => {
                config.rect_x0 = rect.x0 as f32;
                config.rect_y0 = rect.y0 as f32;
                config.rect_x1 = rect.x1 as f32;
                config.rect_y1 = rect.y1 as f32;
                config.radius_top_left = radius.top_left;
                config.radius_top_right = radius.top_right;
                config.radius_bottom_left = radius.bottom_left;
                config.radius_bottom_right = radius.bottom_right;
                rectangle::encode(
                    self.batch,
                    RectanglePass::Mask,
                    config,
                    self.retained
                        .active_tiles()
                        .map(crate::render::damage_tiles::DamageTiles::list),
                    target,
                )
            }
            Region::Path { .. } => {
                config.table_index = path.ok_or("native mask path cursor missing")?;
                path_mask::encode(
                    self.batch,
                    config,
                    self.retained
                        .active_tiles()
                        .map(crate::render::damage_tiles::DamageTiles::list),
                    self.paths.ok_or("native mask paths missing")?,
                    target,
                )
            }
        }
    }
    fn apply_region(
        &mut self,
        mask: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> Result<()> {
        let Some(config) = self.config(bounds) else {
            return Ok(());
        };
        inputs::encode(
            self.batch,
            InputFilter::Mask {
                mask: self.targets.get(mask)?.image(),
            },
            config,
            self.retained
                .active_tiles()
                .map(crate::render::damage_tiles::DamageTiles::list),
            self.targets.get(target)?.image(),
        )
    }
}
