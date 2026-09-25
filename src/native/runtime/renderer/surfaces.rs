use super::*;
use crate::native::runtime::program::filter::{
    self, BasicFilter,
    stack::{Composite, Textures},
};
use crate::render::surfaces::SurfaceAdapter;

impl Execution<'_> {
    fn composite(
        &mut self,
        target: ResourceId,
        source: ResourceId,
        mask: Option<ResourceId>,
        bounds: Bounds,
        stack: Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> Result<()> {
        let Some(mut config) = self.config(bounds) else {
            return Ok(());
        };
        config.layer_stack_start = u32::try_from(stack.start)?;
        config.layer_stack_end = u32::try_from(stack.end)?;
        if let Some(mode) = blend {
            config.blend_mode = mode.mix as u32 | ((mode.compose as u32) << 8);
        }
        self.scene
            .as_ref()
            .ok_or("native scene has not been scanned")?
            .filter_stack()
            .encode(
                self.batch,
                if blend.is_some() {
                    Composite::Blend
                } else {
                    Composite::Over
                },
                config,
                self.retained
                    .active_tiles()
                    .map(crate::render::damage_tiles::DamageTiles::list),
                Textures {
                    source,
                    auxiliary: mask,
                    target,
                },
            )
    }
}

impl SurfaceAdapter for Execution<'_> {
    type Surface = Surface;
    fn size(&self) -> (u32, u32) {
        self.targets.size()
    }
    fn origin(&self) -> (i32, i32) {
        self.origin
    }
    fn retained(&self) -> &RetainedRenderState<Surface> {
        self.retained
    }
    fn retained_mut(&mut self) -> &mut RetainedRenderState<Surface> {
        self.retained
    }
    fn acquire_scratch(&mut self) -> Result<RenderTargetId> {
        self.targets.acquire(self.batch)
    }
    fn install_scratch(&mut self, target: RenderTargetId, surface: Surface) {
        // Shared scheduling only installs a surface from this context's cache.
        self.targets
            .install(self.batch, target, surface)
            .expect("validated native surface context");
    }
    fn take_scratch(&mut self, target: RenderTargetId) -> Option<Surface> {
        self.targets.take(target).ok()
    }
    fn release_scratch(&mut self, target: RenderTargetId) {
        self.targets
            .release(target)
            .expect("occupied native scratch slot");
    }
    fn clear_target(&mut self, target: RenderTargetId) -> Result<()> {
        let (width, height) = self.size();
        filter::encode(
            self.batch,
            BasicFilter::Clear,
            FilterConfig {
                width,
                height,
                region_width: width,
                region_height: height,
                ..Default::default()
            },
            None,
            None,
            self.targets.get(target)?.image(),
        )
    }
    fn clear_region(&mut self, target: RenderTargetId, bounds: Bounds) -> Result<()> {
        let Some(config) = self.config(bounds) else {
            return Ok(());
        };
        filter::encode(
            self.batch,
            BasicFilter::Clear,
            config,
            self.retained
                .active_tiles()
                .map(crate::render::damage_tiles::DamageTiles::list),
            None,
            self.targets.get(target)?.image(),
        )
    }
    fn composite_cached(
        &mut self,
        target: RenderTargetId,
        source: &Surface,
        mask: Option<&Surface>,
        bounds: Bounds,
        stack: Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> Result<()> {
        let source = source.image_in(self.batch)?;
        let mask = mask
            .map(|surface| surface.image_in(self.batch))
            .transpose()?;
        self.composite(
            self.targets.get(target)?.image(),
            source,
            mask,
            bounds,
            stack,
            blend,
        )
    }
    fn composite_targets(
        &mut self,
        target: RenderTargetId,
        source: RenderTargetId,
        mask: RenderTargetId,
        bounds: Bounds,
        stack: Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> Result<()> {
        self.composite(
            self.targets.get(target)?.image(),
            self.targets.get(source)?.image(),
            Some(self.targets.get(mask)?.image()),
            bounds,
            stack,
            blend,
        )
    }
}
