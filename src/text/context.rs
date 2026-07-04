use std::collections::HashMap;

use cosmic_text::{Buffer, CacheKey, FontSystem, Metrics, Shaping};
use peniko::kurbo::{BezPath, Point};
use swash::scale::ScaleContext;

use crate::shared::bounds::Bounds;

use super::{
    layout::{TextGlyph, TextLayout},
    options::{TextLayoutOptions, TextRasterOptions},
    outline::{append_outline_path, outline_cache_key, outline_glyph_path},
    raster::{GlyphRasterImage, RasterGlyphKey, raster_glyph_image},
};

pub struct TextContext {
    scale_context: ScaleContext,
    image_cache: HashMap<RasterGlyphKey, Option<GlyphRasterImage>>,
    outline_cache: HashMap<CacheKey, Option<BezPath>>,
    raster_options: TextRasterOptions,
}

impl TextContext {
    pub fn new() -> Self {
        Self {
            scale_context: ScaleContext::new(),
            image_cache: HashMap::new(),
            outline_cache: HashMap::new(),
            raster_options: TextRasterOptions::default(),
        }
    }

    pub fn raster_options(&self) -> TextRasterOptions {
        self.raster_options
    }

    pub fn set_raster_options(&mut self, options: TextRasterOptions) {
        self.raster_options = options;
    }

    /// Shapes text with the caller-owned font system and caches derived glyph data here.
    ///
    /// `TextContext` does not own `FontSystem`; pass the same font system to
    /// later glyph preparation/rendering calls because cosmic glyph cache keys
    /// contain font-system-local font ids.
    pub fn layout(
        &mut self,
        font_system: &mut FontSystem,
        options: TextLayoutOptions<'_>,
    ) -> TextLayout {
        let metrics = Metrics::new(options.font_size, options.line_height);
        let mut buffer = Buffer::new(font_system, metrics);
        let mut glyphs = Vec::new();

        {
            let mut buffer = buffer.borrow_with(font_system);
            buffer.set_size(options.width, options.height);
            buffer.set_text(
                options.text,
                &options.attrs,
                Shaping::Advanced,
                options.alignment,
            );
            for run in buffer.layout_runs() {
                for glyph in run.glyphs {
                    let outline_origin = Point::new(
                        (glyph.x + glyph.font_size * glyph.x_offset) as f64,
                        (run.line_y + glyph.y - glyph.font_size * glyph.y_offset) as f64,
                    );
                    let physical = glyph.physical((0.0, run.line_y), 1.0);
                    glyphs.push(TextGlyph {
                        cache_key: physical.cache_key,
                        x: physical.x,
                        y: physical.y,
                        outline_origin,
                    });
                }
            }
        }

        let bounds = self.raster_bounds(font_system, &glyphs);
        TextLayout { glyphs, bounds }
    }

    pub(crate) fn glyph_image(
        &mut self,
        font_system: &mut FontSystem,
        cache_key: CacheKey,
    ) -> Option<&GlyphRasterImage> {
        let key = RasterGlyphKey {
            cache_key,
            subpixel_mode: self.raster_options.subpixel_mode,
            embolden_bits: self.raster_options.mask_embolden().to_bits(),
        };
        if !self.image_cache.contains_key(&key) {
            let image = self.raster_glyph_image(font_system, cache_key);
            self.image_cache.insert(key, image);
        }
        self.image_cache.get(&key).and_then(Option::as_ref)
    }

    /// Builds a vector path for every scalable glyph in `layout`.
    ///
    /// The layout still comes from cosmic-text, so shaping, font fallback, and
    /// ligatures are preserved. Glyphs that exist only as bitmap strikes have no
    /// outline and are skipped; render those through
    /// [`crate::Canvas::push_text_layout`] instead of outline text.
    pub fn layout_outline_path(
        &mut self,
        font_system: &mut FontSystem,
        layout: &TextLayout,
        origin: Point,
    ) -> BezPath {
        let mut path = BezPath::new();
        for glyph in layout.glyphs() {
            let Some(outline) = self.glyph_outline_path(font_system, glyph.cache_key) else {
                continue;
            };
            append_outline_path(
                &mut path,
                outline,
                Point::new(
                    origin.x + glyph.outline_origin.x,
                    origin.y + glyph.outline_origin.y,
                ),
            );
        }
        path
    }

    pub(crate) fn glyph_outline_path(
        &mut self,
        font_system: &mut FontSystem,
        cache_key: CacheKey,
    ) -> Option<&BezPath> {
        let key = outline_cache_key(cache_key);
        if !self.outline_cache.contains_key(&key) {
            let outline = outline_glyph_path(font_system, &mut self.scale_context, key);
            self.outline_cache.insert(key, outline);
        }
        self.outline_cache.get(&key).and_then(Option::as_ref)
    }

    fn raster_bounds(&mut self, font_system: &mut FontSystem, glyphs: &[TextGlyph]) -> Bounds {
        let mut bounds = Bounds::new(0, 0, 0, 0);
        for glyph in glyphs {
            let Some(image) = self.glyph_image(font_system, glyph.cache_key) else {
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

    fn raster_glyph_image(
        &mut self,
        font_system: &mut FontSystem,
        cache_key: CacheKey,
    ) -> Option<GlyphRasterImage> {
        raster_glyph_image(
            font_system,
            &mut self.scale_context,
            cache_key,
            self.raster_options.subpixel_mode,
            self.raster_options.mask_embolden(),
        )
        .map(GlyphRasterImage::from_swash)
    }

    /// Clears cached glyph images and outlines after external font database changes.
    pub fn clear_glyph_caches(&mut self) {
        self.image_cache.clear();
        self.outline_cache.clear();
    }
}

impl Default for TextContext {
    fn default() -> Self {
        Self::new()
    }
}
