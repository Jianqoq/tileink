use cosmic_text::{CacheKey, SubpixelBin};
use peniko::kurbo::Point;

use crate::shared::bounds::Bounds;

use super::raster::GlyphRasterImage;

#[derive(Clone, Debug)]
pub struct TextLayout {
    pub(crate) glyphs: Vec<TextGlyph>,
    pub(crate) bounds: Bounds,
}

impl TextLayout {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.glyphs.is_empty() || self.bounds.is_empty()
    }

    #[must_use]
    pub const fn bounds(&self) -> Bounds {
        self.bounds
    }

    pub(crate) fn glyphs(&self) -> &[TextGlyph] {
        &self.glyphs
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextGlyph {
    pub(crate) cache_key: CacheKey,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) outline_origin: Point,
}

impl TextGlyph {
    fn at_scaled_origin(self, origin: Point, scale: f32) -> CanvasGlyph {
        let cache_key = CacheKey {
            font_size_bits: (f32::from_bits(self.cache_key.font_size_bits) * scale).to_bits(),
            ..self.cache_key
        };
        CanvasGlyph::at_position(
            cache_key,
            (f64::from(self.x) + f64::from(self.cache_key.x_bin.as_float()) + origin.x)
                * f64::from(scale),
            (f64::from(self.y) + f64::from(self.cache_key.y_bin.as_float()) + origin.y)
                * f64::from(scale),
        )
    }

    pub(crate) fn image_bounds(self, image: &GlyphRasterImage) -> Bounds {
        glyph_image_bounds(
            self.x,
            self.y,
            image.placement.left,
            image.placement.top,
            image.placement.width,
            image.placement.height,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CanvasGlyph {
    pub(crate) cache_key: CacheKey,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl CanvasGlyph {
    pub(super) fn at_position(cache_key: CacheKey, x: f64, y: f64) -> Self {
        let (x, x_bin) = split_position(x);
        let (y, y_bin) = split_position(y);
        Self {
            cache_key: CacheKey {
                x_bin,
                y_bin,
                ..cache_key
            },
            x,
            y,
        }
    }

    pub(crate) fn translated(self, dx: f64, dy: f64) -> Self {
        Self::at_position(
            self.cache_key,
            f64::from(self.x) + f64::from(self.cache_key.x_bin.as_float()) + dx,
            f64::from(self.y) + f64::from(self.cache_key.y_bin.as_float()) + dy,
        )
    }
}

// Only the fractional part reaches cosmic-text's binning arithmetic. Passing a
// saturated full position can overflow its +/-1 carry; f64 also preserves small
// offsets when large integer translations cancel. Clamping before splitting
// guarantees that the carry still fits the signed coordinate domain.
fn split_position(position: f64) -> (i32, SubpixelBin) {
    let position = position.clamp(f64::from(i32::MIN), f64::from(i32::MAX));
    let (carry, bin) = SubpixelBin::new(position.fract() as f32);
    (position.trunc() as i32 + carry, bin)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TextRun {
    pub(crate) glyph_start: u32,
    pub(crate) glyph_count: u32,
}

#[cfg(test)]
pub(crate) fn scene_glyphs_at_origin(
    layout: &TextLayout,
    origin: Point,
) -> impl Iterator<Item = CanvasGlyph> + '_ {
    scene_glyphs_at_scaled_origin(layout, origin, 1.0)
}

pub(crate) fn scene_glyphs_at_scaled_origin(
    layout: &TextLayout,
    origin: Point,
    scale: f32,
) -> impl Iterator<Item = CanvasGlyph> + '_ {
    layout
        .glyphs()
        .iter()
        .copied()
        .map(move |glyph| glyph.at_scaled_origin(origin, scale))
}

pub(crate) fn layout_bounds_at_scaled_origin(
    layout: &TextLayout,
    origin: Point,
    scale: f32,
) -> Bounds {
    let bounds = layout.bounds;
    if bounds.is_empty() {
        return bounds;
    }
    // Keep translation, DPI and raster padding wide until the final endpoints.
    // Clamping before scaling loses large logical origins that become visible
    // at small DPI. One local pixel covers cache-bin shifts; scaled hinting adds
    // the existing ceil(scale)+2 physical support.
    let scale = f64::from(scale);
    let pad = if scale == 1.0 {
        0.0
    } else {
        scale.ceil() + 2.0
    };
    let lower = |edge: i32, origin: f64| {
        (((f64::from(edge) + origin.floor() - 1.0) * scale).floor() - pad) as i32
    };
    let upper = |edge: i32, origin: f64| {
        (((f64::from(edge) + origin.ceil() + 1.0) * scale).ceil() + pad) as i32
    };
    Bounds::new(
        lower(bounds.x0, origin.x),
        lower(bounds.y0, origin.y),
        upper(bounds.x1, origin.x),
        upper(bounds.y1, origin.y),
    )
}

/// Raster and prepared images share one endpoint calculation. Clamp after the
/// complete sum so bearings outside the domain cannot erase the visible tail.
pub(super) fn glyph_image_bounds(
    x: i32,
    y: i32,
    left: i32,
    top: i32,
    width: u32,
    height: u32,
) -> Bounds {
    let left = i64::from(x) + i64::from(left);
    let top = i64::from(y) - i64::from(top);
    let clamp = |value: i64| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    Bounds::new(
        clamp(left),
        clamp(top),
        clamp(left + i64::from(width)),
        clamp(top + i64::from(height)),
    )
}
