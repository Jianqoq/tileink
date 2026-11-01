use super::{TextSubpixelMode, raster::*};
use cosmic_text::{CacheKey, FontSystem};
use swash::scale::ScaleContext;
#[cfg(target_os = "windows")]
mod windows;

/// Owns both raster adapters and their per-context font resources. Image ownership
/// stays in `TextContext`'s existing cache, shared by measurement and preparation.
pub(super) struct GlyphRasterizer {
    pub(super) scale_context: ScaleContext,
    #[cfg(target_os = "windows")]
    native: windows::Backend,
}
impl GlyphRasterizer {
    pub(super) fn new() -> Self {
        Self {
            scale_context: ScaleContext::new(),
            #[cfg(target_os = "windows")]
            native: windows::Backend::default(),
        }
    }
    pub(super) fn clear(&mut self) {
        self.scale_context = ScaleContext::new();
        #[cfg(target_os = "windows")]
        self.native.clear();
    }
    pub(super) fn image(
        &mut self,
        fonts: &mut FontSystem,
        key: CacheKey,
        mode: TextSubpixelMode,
        embolden: f32,
    ) -> Option<GlyphRasterImage> {
        #[cfg(target_os = "windows")]
        if embolden.max(0.0) == 0.0 {
            // Preserve the established color-outline/bitmap priority. Explicit
            // outline expansion uses the portable adapter that implements it.
            if let Some(color) = raster_color_glyph_image(fonts, &mut self.scale_context, key, 0.0)
            {
                return Some(color);
            }
            if let Ok(image) = self.native.image(fonts, key, mode) {
                return image;
            }
        }
        raster_glyph_image(fonts, &mut self.scale_context, key, mode, embolden)
    }
}
