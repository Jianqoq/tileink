use std::collections::HashMap;

use cosmic_text::{Buffer, CacheKey, FontSystem, Metrics, Shaping, SubpixelBin};
use peniko::kurbo::{BezPath, Point};
use swash::scale::ScaleContext;

use self::glyph::CachedGlyph;

use crate::shared::bounds::Bounds;

use super::{
    layout::{CanvasGlyph, TextGlyph, TextLayout},
    options::{TextLayoutOptions, TextRasterOptions},
    outline::{append_outline_path, outline_cache_key, outline_glyph_path},
    raster::RasterGlyphKey,
};

mod glyph;
mod ink;

pub struct TextContext {
    scale_context: ScaleContext,
    image_cache: HashMap<RasterGlyphKey, Option<CachedGlyph>>,
    outline_cache: HashMap<CacheKey, Option<BezPath>>,
    raster_options: TextRasterOptions,
    cache_generation: u64,
}

impl TextContext {
    #[must_use]
    pub fn new() -> Self {
        Self {
            scale_context: ScaleContext::new(),
            image_cache: HashMap::new(),
            outline_cache: HashMap::new(),
            raster_options: TextRasterOptions::default(),
            cache_generation: 0,
        }
    }

    #[must_use]
    pub const fn raster_options(&self) -> TextRasterOptions {
        self.raster_options
    }

    pub const fn set_raster_options(&mut self, options: TextRasterOptions) {
        self.raster_options = options;
    }

    pub(crate) const fn cache_generation(&self) -> u64 {
        self.cache_generation
    }

    /// Shapes text with the caller-owned font system and caches derived glyph data here.
    ///
    /// `TextContext` does not own `FontSystem`; pass the same font system to
    /// later glyph preparation/rendering calls because cosmic glyph cache keys
    /// contain font-system-local font ids.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "Shaping consumes temporary layout options and releases their owned font-feature storage"
    )]
    pub fn layout(
        &mut self,
        font_system: &mut FontSystem,
        options: TextLayoutOptions<'_>,
    ) -> TextLayout {
        let metrics = Metrics::new(options.font_size, options.line_height);
        let mut buffer = Buffer::new(font_system, metrics);

        {
            let mut buffer = buffer.borrow_with(font_system);
            buffer.set_size(options.width, options.height);
            buffer.set_wrap(options.wrap);
            buffer.set_text(
                options.text,
                &options.attrs,
                Shaping::Advanced,
                options.alignment,
            );
        }
        self.layout_buffer(font_system, &mut buffer)
    }

    /// Builds a Tileink layout from a retained Cosmic Text buffer.
    ///
    /// UI frameworks that retain an editable or measured buffer should use this path; allocating
    /// and shaping a second buffer would duplicate the most expensive part of responsive text
    /// recording. Pending buffer changes are shaped before glyph extraction, while an unchanged
    /// already-shaped buffer stays on Cosmic Text's inexpensive reuse path. `buffer` must have
    /// been created with `font_system` because Cosmic Text's font ids are local to that system.
    #[allow(
        clippy::suspicious_operation_groupings,
        reason = "Glyph placement combines the run baseline with the glyph offset in the same axis"
    )]
    pub fn layout_buffer(
        &mut self,
        font_system: &mut FontSystem,
        buffer: &mut Buffer,
    ) -> TextLayout {
        buffer.shape_until_scroll(font_system, false);
        let mut glyphs = Vec::new();
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let outline_origin = Point::new(
                    f64::from(glyph.x + glyph.font_size * glyph.x_offset),
                    f64::from(run.line_y + glyph.y - glyph.font_size * glyph.y_offset),
                );
                // Preserve Cosmic Text's Y hinting and key metadata, but use
                // the same bounded position conversion as recording/append.
                // physical() passes full positions to its overflowing bin carry.
                let physical = CanvasGlyph::at_position(
                    CacheKey {
                        font_id: glyph.font_id,
                        glyph_id: glyph.glyph_id,
                        font_size_bits: glyph.font_size.to_bits(),
                        x_bin: SubpixelBin::Zero,
                        y_bin: SubpixelBin::Zero,
                        font_weight: glyph.font_weight,
                        flags: glyph.cache_key_flags,
                    },
                    f64::from(glyph.x + glyph.font_size * glyph.x_offset),
                    f64::from(
                        (glyph.y - glyph.font_size * glyph.y_offset)
                            .mul_add(1.0, run.line_y)
                            .trunc(),
                    ),
                );
                glyphs.push(TextGlyph {
                    cache_key: physical.cache_key,
                    x: physical.x,
                    y: physical.y,
                    outline_origin,
                });
            }
        }
        let bounds = self.raster_bounds(font_system, &glyphs);
        TextLayout { glyphs, bounds }
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
        self.outline_cache
            .entry(key)
            .or_insert_with(|| outline_glyph_path(font_system, &mut self.scale_context, key))
            .as_ref()
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

    /// Clears cached glyph images and outlines after external font database changes.
    pub fn clear_glyph_caches(&mut self) {
        self.image_cache.clear();
        self.outline_cache.clear();
        // Prepared renderer text must not keep pixels derived from the previous font database.
        self.cache_generation = self.cache_generation.wrapping_add(1);
    }
}

impl Default for TextContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
