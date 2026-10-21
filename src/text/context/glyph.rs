//! Pixel data and visible coverage share one font/DPI/raster-keyed cache entry.
use super::*;
use crate::text::raster::{GlyphContent, GlyphRasterImage, raster_glyph_image};
use peniko::kurbo::Rect;

pub(super) struct CachedGlyph {
    pub image: GlyphRasterImage,
    pub ink: Option<Rect>,
}

impl TextContext {
    pub(super) fn glyph(
        &mut self,
        fonts: &mut FontSystem,
        cache_key: CacheKey,
    ) -> Option<&CachedGlyph> {
        let key = RasterGlyphKey {
            cache_key,
            subpixel_mode: self.raster_options.subpixel_mode,
            embolden_bits: self.raster_options.mask_embolden().to_bits(),
        };
        self.image_cache
            .entry(key)
            .or_insert_with(|| {
                let image = raster_glyph_image(
                    fonts,
                    &mut self.scale_context,
                    cache_key,
                    key.subpixel_mode,
                    f32::from_bits(key.embolden_bits),
                )?;
                // Compute after LCD filtering and all raster transforms, not on intermediate masks.
                // Recording/measurement and atlas preparation then reuse the very same pixels.
                let ink = visible_bounds(&image);
                Some(CachedGlyph { image, ink })
            })
            .as_ref()
    }

    pub(crate) fn glyph_image(
        &mut self,
        fonts: &mut FontSystem,
        cache_key: CacheKey,
    ) -> Option<&GlyphRasterImage> {
        self.glyph(fonts, cache_key).map(|glyph| &glyph.image)
    }
}

fn visible_bounds(image: &GlyphRasterImage) -> Option<Rect> {
    let channels = match image.content {
        GlyphContent::Mask => 1,
        GlyphContent::SubpixelMask => 3,
        GlyphContent::Color => 4,
    };
    let width = image.placement.width as usize;
    if width == 0 {
        return None;
    }
    let mut left = width;
    let mut top = image.placement.height as usize;
    let mut right = 0;
    let mut bottom = 0;
    for (y, row) in image.data.chunks_exact(width * channels).enumerate() {
        for (x, pixel) in row.chunks_exact(channels).enumerate() {
            let visible = if image.content == GlyphContent::Color {
                pixel[3] != 0
            } else {
                pixel.iter().any(|&v| v != 0)
            };
            if visible {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }
    (right > left).then(|| {
        Rect::new(
            left as f64 + f64::from(image.placement.left),
            top as f64 - f64::from(image.placement.top),
            right as f64 + f64::from(image.placement.left),
            bottom as f64 - f64::from(image.placement.top),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use swash::zeno::Placement;

    #[test]
    fn visible_bounds_exclude_padding_and_use_color_alpha() {
        for content in [
            GlyphContent::Mask,
            GlyphContent::SubpixelMask,
            GlyphContent::Color,
        ] {
            let channels = match content {
                GlyphContent::Mask => 1,
                GlyphContent::SubpixelMask => 3,
                GlyphContent::Color => 4,
            };
            let mut data = vec![0; 4 * 5 * channels];
            data[(2 * 4 + 1) * channels + if content == GlyphContent::Color { 3 } else { 0 }] = 255;
            let image = GlyphRasterImage::new(
                content,
                Placement {
                    left: 3,
                    top: 8,
                    width: 4,
                    height: 5,
                },
                data,
            );
            assert_eq!(
                visible_bounds(&image),
                Some(Rect::new(4.0, -6.0, 5.0, -5.0))
            );
            let transparent =
                GlyphRasterImage::new(content, image.placement, vec![0; image.data.len()]);
            assert_eq!(visible_bounds(&transparent), None);
        }
        let empty = GlyphRasterImage::new(GlyphContent::Mask, Placement::default(), vec![]);
        assert_eq!(visible_bounds(&empty), None);
        // RGB bytes in a transparent color image must not enlarge its visible coverage.
        let transparent = GlyphRasterImage::new(
            GlyphContent::Color,
            Placement {
                width: 1,
                height: 1,
                ..Placement::default()
            },
            vec![255, 255, 255, 0],
        );
        assert_eq!(visible_bounds(&transparent), None);
    }

    #[test]
    fn measurement_and_render_preparation_share_raster_entries() {
        let mut fonts = crate::text::tests::font_system();
        let mut context = TextContext::new();
        let layout = context.layout(&mut fonts, TextLayoutOptions::new("B 1 @ 123.45", 12.0));
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            context.clear_glyph_caches();
            let origin = Point::new(20.0, 20.0);
            let measured = context.ink_bounds_at_scale(&mut fonts, &layout, origin, scale);
            assert!(measured.is_some());
            let before: HashMap<_, _> = context
                .image_cache
                .iter()
                .map(|(key, glyph)| (*key, glyph.as_ref().map(|g| (g.image.data.as_ptr(), g.ink))))
                .collect();
            let glyphs: Vec<_> =
                crate::text::scene_glyphs_at_scaled_origin(&layout, origin, scale).collect();
            let _prepared =
                crate::text::PreparedTextData::new(&glyphs, &[], &mut fonts, &mut context);
            assert_eq!(context.image_cache.len(), before.len());
            for (key, glyph) in &context.image_cache {
                assert_eq!(
                    glyph.as_ref().map(|g| (g.image.data.as_ptr(), g.ink)),
                    before[key]
                );
            }
            assert_eq!(
                context.ink_bounds_at_scale(&mut fonts, &layout, origin, scale),
                measured
            );
        }
    }
}
