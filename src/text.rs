use std::collections::HashMap;

use cosmic_text::{
    Align, Attrs, Buffer, CacheKey, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
    SwashImage,
};
use peniko::kurbo::Point;

use crate::shared::bounds::Bounds;

#[derive(Clone, Debug)]
pub struct TextLayoutOptions<'a> {
    pub text: &'a str,
    pub font_size: f32,
    pub line_height: f32,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub attrs: Attrs<'a>,
    pub alignment: Option<Align>,
}

impl<'a> TextLayoutOptions<'a> {
    pub fn new(text: &'a str, font_size: f32) -> Self {
        Self {
            text,
            font_size,
            line_height: font_size * 1.2,
            width: None,
            height: None,
            attrs: Attrs::new(),
            alignment: None,
        }
    }

    pub fn with_size(mut self, width: Option<f32>, height: Option<f32>) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    pub fn with_attrs(mut self, attrs: Attrs<'a>) -> Self {
        self.attrs = attrs;
        self
    }

    pub fn with_alignment(mut self, alignment: Option<Align>) -> Self {
        self.alignment = alignment;
        self
    }
}

pub struct TextContext {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl TextContext {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        }
    }

    pub fn layout(&mut self, options: TextLayoutOptions<'_>) -> TextLayout {
        let metrics = Metrics::new(options.font_size, options.line_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let mut glyphs = Vec::new();

        {
            let mut buffer = buffer.borrow_with(&mut self.font_system);
            buffer.set_size(options.width, options.height);
            buffer.set_text(
                options.text,
                &options.attrs,
                Shaping::Advanced,
                options.alignment,
            );
            for run in buffer.layout_runs() {
                for glyph in run.glyphs {
                    let physical = glyph.physical((0.0, run.line_y), 1.0);
                    glyphs.push(TextGlyph {
                        cache_key: physical.cache_key,
                        x: physical.x,
                        y: physical.y,
                    });
                }
            }
        }

        let bounds = self.raster_bounds(&glyphs);
        TextLayout { glyphs, bounds }
    }

    pub(crate) fn glyph_image(&mut self, cache_key: CacheKey) -> Option<&SwashImage> {
        self.swash_cache
            .get_image(&mut self.font_system, cache_key)
            .as_ref()
    }

    fn raster_bounds(&mut self, glyphs: &[TextGlyph]) -> Bounds {
        let mut bounds = Bounds::new(0, 0, 0, 0);
        for glyph in glyphs {
            let Some(image) = self.glyph_image(glyph.cache_key) else {
                continue;
            };
            let glyph_bounds = glyph.image_bounds(image);
            bounds = if bounds.is_empty() {
                glyph_bounds
            } else {
                bounds.union(glyph_bounds)
            };
        }
        bounds
    }
}

impl Default for TextContext {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct TextLayout {
    glyphs: Vec<TextGlyph>,
    bounds: Bounds,
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
    cache_key: CacheKey,
    x: i32,
    y: i32,
}

impl TextGlyph {
    fn at_origin(self, origin: Point) -> SceneGlyph {
        let (cache_key, x, y) = translated_cache_key(self.cache_key, self.x, self.y, origin);
        SceneGlyph { cache_key, x, y }
    }

    fn image_bounds(self, image: &SwashImage) -> Bounds {
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
pub(crate) struct SceneGlyph {
    pub(crate) cache_key: CacheKey,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl SceneGlyph {
    pub(crate) fn translated(self, dx: i32, dy: i32) -> Self {
        let (cache_key, x, y) = translated_cache_key(
            self.cache_key,
            self.x,
            self.y,
            Point::new(dx as f64, dy as f64),
        );
        Self { cache_key, x, y }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextRun {
    pub(crate) glyph_start: u32,
    pub(crate) glyph_count: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedTextData {
    runs: Vec<TextRun>,
    glyphs: Vec<PreparedGlyph>,
    images: Vec<PreparedGlyphImage>,
    image_by_key: HashMap<CacheKey, u32>,
}

impl PreparedTextData {
    pub(crate) fn new(glyphs: &[SceneGlyph], runs: &[TextRun], context: &mut TextContext) -> Self {
        let mut image_by_key = HashMap::new();
        let mut images = Vec::new();
        let mut prepared_glyphs = Vec::with_capacity(glyphs.len());

        for glyph in glyphs {
            let image = if let Some(&image) = image_by_key.get(&glyph.cache_key) {
                Some(image)
            } else if let Some(image) = context.glyph_image(glyph.cache_key) {
                let image_ix = images.len() as u32;
                images.push(PreparedGlyphImage::from_swash(image));
                image_by_key.insert(glyph.cache_key, image_ix);
                Some(image_ix)
            } else {
                None
            };
            prepared_glyphs.push(PreparedGlyph {
                image,
                x: glyph.x,
                y: glyph.y,
            });
        }

        Self {
            runs: runs.to_vec(),
            glyphs: prepared_glyphs,
            images,
            image_by_key,
        }
    }

    pub(crate) fn run_glyphs(&self, run_id: u32) -> &[PreparedGlyph] {
        let Some(run) = self.runs.get(run_id as usize) else {
            return &[];
        };
        let start = run.glyph_start as usize;
        let end = start.saturating_add(run.glyph_count as usize);
        self.glyphs.get(start..end).unwrap_or(&[])
    }

    pub(crate) fn image(&self, image_id: u32) -> Option<&PreparedGlyphImage> {
        self.images.get(image_id as usize)
    }

    pub(crate) fn image_id_for_cache_key(&self, cache_key: CacheKey) -> Option<u32> {
        self.image_by_key.get(&cache_key).copied()
    }

    pub(crate) fn images(&self) -> &[PreparedGlyphImage] {
        &self.images
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedGlyph {
    pub(crate) image: Option<u32>,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedGlyphImage {
    pub(crate) content: PreparedGlyphContent,
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data: Vec<u8>,
}

impl PreparedGlyphImage {
    fn from_swash(image: &SwashImage) -> Self {
        Self {
            content: match image.content {
                SwashContent::Mask => PreparedGlyphContent::Mask,
                SwashContent::Color => PreparedGlyphContent::Color,
                SwashContent::SubpixelMask => PreparedGlyphContent::SubpixelMask,
            },
            left: image.placement.left,
            top: image.placement.top,
            width: image.placement.width,
            height: image.placement.height,
            data: image.data.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreparedGlyphContent {
    Mask,
    Color,
    SubpixelMask,
}

pub(crate) fn scene_glyphs_at_origin<'a>(
    layout: &'a TextLayout,
    origin: Point,
) -> impl Iterator<Item = SceneGlyph> + 'a {
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
    // extents by one pixel, so the scene bounds are deliberately conservative.
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
    image: &SwashImage,
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

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_text::Weight;

    #[test]
    fn layout_produces_positioned_glyphs_when_a_font_is_available() {
        let mut context = TextContext::new();
        let layout = context.layout(TextLayoutOptions::new("Hello", 24.0));
        if layout.glyphs.is_empty() {
            return;
        }

        assert!(!layout.bounds().is_empty());
    }

    #[test]
    fn scene_glyph_translation_recomputes_subpixel_cache_key() {
        let mut context = TextContext::new();
        let layout = context.layout(TextLayoutOptions::new("A", 20.0));
        if layout.glyphs.is_empty() {
            return;
        }

        let a = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0))
            .next()
            .unwrap();
        let b = scene_glyphs_at_origin(&layout, Point::new(0.5, 0.0))
            .next()
            .unwrap();
        assert_ne!(a.cache_key, b.cache_key);
    }

    #[test]
    fn layout_options_pass_cosmic_attrs_to_shaping() {
        let mut context = TextContext::new();
        let layout = context.layout(
            TextLayoutOptions::new("A", 20.0).with_attrs(Attrs::new().weight(Weight::BOLD)),
        );
        if layout.glyphs.is_empty() {
            return;
        }

        assert_eq!(layout.glyphs[0].cache_key.font_weight, Weight::BOLD);
    }

    #[test]
    fn layout_options_pass_alignment_to_cosmic_buffer() {
        let mut context = TextContext::new();
        let left = context.layout(TextLayoutOptions::new("A", 20.0).with_size(Some(200.0), None));
        let center = context.layout(
            TextLayoutOptions::new("A", 20.0)
                .with_size(Some(200.0), None)
                .with_alignment(Some(Align::Center)),
        );
        if left.glyphs.is_empty() || center.glyphs.is_empty() {
            return;
        }

        assert!(center.glyphs[0].x > left.glyphs[0].x);
    }
}
