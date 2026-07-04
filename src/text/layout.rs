use cosmic_text::CacheKey;
use peniko::kurbo::Point;

use crate::shared::bounds::Bounds;

use super::raster::GlyphRasterImage;

#[derive(Clone, Debug)]
pub struct TextLayout {
    pub(crate) glyphs: Vec<TextGlyph>,
    pub(crate) bounds: Bounds,
}

impl TextLayout {
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty() || self.bounds.is_empty()
    }

    pub fn bounds(&self) -> Bounds {
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
    fn at_origin(self, origin: Point) -> CanvasGlyph {
        let (cache_key, x, y) = translated_cache_key(self.cache_key, self.x, self.y, origin);
        CanvasGlyph { cache_key, x, y }
    }

    pub(crate) fn image_bounds(self, image: &GlyphRasterImage) -> Bounds {
        glyph_image_bounds(
            self.x,
            self.y,
            image.placement.left,
            image.placement.top,
            image,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CanvasGlyph {
    pub(crate) cache_key: CacheKey,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl CanvasGlyph {
    pub(crate) fn translated(self, dx: f64, dy: f64) -> Self {
        let (cache_key, x, y) =
            translated_cache_key(self.cache_key, self.x, self.y, Point::new(dx, dy));
        Self { cache_key, x, y }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextRun {
    pub(crate) glyph_start: u32,
    pub(crate) glyph_count: u32,
}

pub(crate) fn scene_glyphs_at_origin<'a>(
    layout: &'a TextLayout,
    origin: Point,
) -> impl Iterator<Item = CanvasGlyph> + 'a {
    layout
        .glyphs()
        .iter()
        .copied()
        .map(move |glyph| glyph.at_origin(origin))
}

pub(crate) fn layout_bounds_at_origin(layout: &TextLayout, origin: Point) -> Bounds {
    if layout.bounds.is_empty() {
        return layout.bounds;
    }
    // Fractional origins can change the subpixel cache bin and shift raster
    // extents by one pixel, so the canvas bounds are deliberately conservative.
    Bounds::new(
        layout.bounds.x0 + origin.x.floor() as i32 - 1,
        layout.bounds.y0 + origin.y.floor() as i32 - 1,
        layout.bounds.x1 + origin.x.ceil() as i32 + 1,
        layout.bounds.y1 + origin.y.ceil() as i32 + 1,
    )
}

fn translated_cache_key(
    cache_key: CacheKey,
    x: i32,
    y: i32,
    origin: Point,
) -> (CacheKey, i32, i32) {
    CacheKey::new(
        cache_key.font_id,
        cache_key.glyph_id,
        f32::from_bits(cache_key.font_size_bits),
        (
            x as f32 + cache_key.x_bin.as_float() + origin.x as f32,
            y as f32 + cache_key.y_bin.as_float() + origin.y as f32,
        ),
        cache_key.font_weight,
        cache_key.flags,
    )
}

fn glyph_image_bounds(
    x: i32,
    y: i32,
    placement_left: i32,
    placement_top: i32,
    image: &GlyphRasterImage,
) -> Bounds {
    let left = x + placement_left;
    let top = y - placement_top;
    Bounds::new(
        left,
        top,
        left + image.placement.width as i32,
        top + image.placement.height as i32,
    )
}
