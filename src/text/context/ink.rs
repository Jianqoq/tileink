//! Visible glyph coverage at the actual drawing scale, excluding transparent raster padding.
use super::*;
use crate::text::layout::scene_glyphs_at_scaled_origin;
use peniko::kurbo::Rect;

impl TextContext {
    /// Measures nontransparent glyph pixels in logical coordinates at the canvas scale/origin.
    /// Unlike `TextLayout::bounds`, this measures the rasterized drawing at its actual DPI.
    /// Returns `None` for empty/whitespace text or invalid coordinates/scale. It does not alter
    /// layout, renderer damage bounds, glyph sampling or SVG output.
    pub fn ink_bounds_at_scale(
        &mut self,
        fonts: &mut FontSystem,
        layout: &TextLayout,
        origin: Point,
        scale: f32,
    ) -> Option<Rect> {
        if !origin.is_finite() || !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let mut bounds: Option<Rect> = None;
        for glyph in scene_glyphs_at_scaled_origin(layout, origin, scale) {
            let ink = self
                .glyph(fonts, glyph.cache_key)
                .and_then(|glyph| glyph.ink);
            let Some(ink) = ink else {
                continue;
            };
            let rect = ink + peniko::kurbo::Vec2::new(f64::from(glyph.x), f64::from(glyph.y));
            bounds = Some(bounds.map_or(rect, |b| b.union(rect)));
        }
        bounds.map(|b| {
            let scale = f64::from(scale);
            Rect::new(b.x0 / scale, b.y0 / scale, b.x1 / scale, b.y1 / scale)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_ink_tracks_dpi_origin_and_cache_reset() {
        let mut fonts = crate::text::tests::font_system();
        let mut context = TextContext::new();
        let text = context.layout(&mut fonts, crate::TextLayoutOptions::new("B 123.45", 12.0));
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let b = context
                .ink_bounds_at_scale(&mut fonts, &text, Point::ZERO, scale)
                .unwrap();
            let translated = context
                .ink_bounds_at_scale(&mut fonts, &text, Point::new(20.0, 12.0), scale)
                .unwrap();
            assert!((translated.x0 - b.x0 - 20.0).abs() <= 1.0 / f64::from(scale));
            assert!((translated.y0 - b.y0 - 12.0).abs() <= 1.0 / f64::from(scale));
            context.clear_glyph_caches();
            assert_eq!(
                context.ink_bounds_at_scale(&mut fonts, &text, Point::ZERO, scale),
                Some(b)
            );
        }
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                context
                    .ink_bounds_at_scale(&mut fonts, &text, Point::ZERO, scale)
                    .is_none()
            );
        }
        let blank = context.layout(&mut fonts, crate::TextLayoutOptions::new("   ", 12.0));
        assert!(
            context
                .ink_bounds_at_scale(&mut fonts, &blank, Point::ZERO, 2.0)
                .is_none()
        );
    }
}
