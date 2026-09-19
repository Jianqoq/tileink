use super::*;
use crate::native::runtime::program::filter::{self, BasicFilter};
use crate::render::groups::GroupAdapter;

impl GroupAdapter for Execution<'_> {
    fn apply_opacity(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        opacity: f32,
    ) -> Result<()> {
        let Some(mut config) = self.config(bounds) else {
            return Ok(());
        };
        config.filter_kind = crate::shared::gpu_constants::FILTER_OPACITY;
        config.amount = opacity;
        filter::encode(
            self.batch,
            BasicFilter::Color,
            config,
            None,
            None,
            self.targets.get(target)?.image(),
        )
    }
    fn build_mask(&mut self, target: RenderTargetId, draw: u32, bounds: Bounds) -> Result<()> {
        let Some(mut config) = self.config(bounds) else {
            return Ok(());
        };
        config.draw_ix = draw;
        self.scene
            .as_ref()
            .ok_or("native scene has not been scanned")?
            .filter_geometry()
            .mask(self.batch, config, None, self.targets.get(target)?.image())
    }
}
