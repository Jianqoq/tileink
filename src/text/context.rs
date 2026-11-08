use super::rasterizer::GlyphRasterizer;
use std::collections::HashMap;

use cosmic_text::{Buffer, CacheKey, FontSystem, Metrics, Shaping, SubpixelBin};
use peniko::kurbo::{BezPath, Point};

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
    rasterizer: GlyphRasterizer,
    image_cache: HashMap<RasterGlyphKey, Option<CachedGlyph>>,
    outline_cache: HashMap<CacheKey, Option<BezPath>>,
    raster_options: TextRasterOptions,
    cache_generation: u64,
}

// One conversion owns physical cache keys for uniform and span-aware recording.
// Extracting prepared runs must never quantize a baseline differently or reshape their source.
fn prepared_glyph(glyph: &cosmic_text::LayoutGlyph, line_y: f32) -> TextGlyph {
    let offset_y = glyph.y - glyph.font_size * glyph.y_offset;
    let outline_origin = Point::new(
        f64::from(glyph.x + glyph.font_size * glyph.x_offset),
        f64::from(line_y + offset_y),
    );
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
        outline_origin.x,
        outline_origin.y,
    );
    TextGlyph {
        cache_key: physical.cache_key,
        x: physical.x,
        y: physical.y,
        outline_origin,
    }
}

impl TextContext {
    #[must_use]
    pub fn new() -> Self {
        Self {
            rasterizer: GlyphRasterizer::new(),
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

    /// Builds a uniform-brush layout from a retained Cosmic Text buffer.
    /// Pending changes are shaped once; prepared buffers use Cosmic Text's reuse path.
    /// Use `layout_buffer_runs` for independent colors or interactions on prepared spans.
    /// The buffer must have been created with `font_system`; font ids are local to that system.
    pub fn layout_buffer(
        &mut self,
        font_system: &mut FontSystem,
        buffer: &mut Buffer,
    ) -> TextLayout {
        buffer.shape_until_scroll(font_system, false);
        let glyphs = super::text_layout_runs(buffer)
            .flat_map(|run| {
                run.glyphs
                    .iter()
                    .map(move |glyph| prepared_glyph(glyph, run.line_y))
            })
            .collect::<Vec<_>>();
        let bounds = self.raster_bounds(font_system, &glyphs);
        TextLayout { glyphs, bounds }
    }
    /// Extracts ordered paint runs from an already-shaped buffer without mutating or reshaping it.
    /// The buffer and its glyph font ids must belong to `font_system`. Each run remains on one
    /// visual line and has one metadata/color value; noncontiguous values remain separate so
    /// overlapping glyphs keep their source paint order. Empty paragraphs retain their line
    /// indices but produce no paint run. Baselines use the same UI metrics as `layout_buffer`.
    pub fn layout_buffer_runs(
        &mut self,
        font_system: &mut FontSystem,
        buffer: &Buffer,
    ) -> Vec<super::layout::TextLayoutRun> {
        self.layout_buffer_runs_filtered(font_system, buffer, |_, _| true)
    }

    /// Projects already-shaped glyphs into paint runs without modifying their source or positions.
    /// The predicate receives the visual-line index (including empty paragraphs) and each original
    /// glyph exactly once. Callers own cluster-level inclusion: include all glyphs of a cluster
    /// together when implementing truncation. Rejected glyphs leave no paint; empty groups are
    /// omitted. The remaining metadata/color groups retain their original order and UI baselines.
    pub fn layout_buffer_runs_filtered(
        &mut self,
        font_system: &mut FontSystem,
        buffer: &Buffer,
        include: impl FnMut(usize, &cosmic_text::LayoutGlyph) -> bool,
    ) -> Vec<super::layout::TextLayoutRun> {
        self.layout_runs_filtered(font_system, super::text_layout_runs(buffer), include)
    }

    /// Prepares paint spans from caller-provided, already-shaped UI lines without reshaping them.
    ///
    /// Keep the source byte indices and font-system-local glyph ids valid. Each visible visual
    /// line, including empty ones, retains its ordinal; metadata/color groups and cluster inclusion
    /// have the same contract as `layout_buffer_runs_filtered`. The supplied UI baselines and glyph
    /// positions are authoritative for raster and outline preparation.
    pub fn layout_runs_filtered<'a>(
        &mut self,
        font_system: &mut FontSystem,
        runs: impl IntoIterator<Item = cosmic_text::LayoutRun<'a>>,
        mut include: impl FnMut(usize, &cosmic_text::LayoutGlyph) -> bool,
    ) -> Vec<super::layout::TextLayoutRun> {
        let mut result = Vec::new();
        for (line, run) in runs.into_iter().enumerate() {
            let mut start = 0;
            while start < run.glyphs.len() {
                let metadata = run.glyphs[start].metadata;
                let color = run.glyphs[start].color_opt;
                let mut end = start + 1;
                while end < run.glyphs.len()
                    && run.glyphs[end].metadata == metadata
                    && run.glyphs[end].color_opt == color
                {
                    end += 1;
                }
                let glyphs = run.glyphs[start..end]
                    .iter()
                    .filter(|glyph| include(line, glyph))
                    .map(|glyph| prepared_glyph(glyph, run.line_y))
                    .collect::<Vec<_>>();
                if glyphs.is_empty() {
                    start = end;
                    continue;
                }
                let bounds = self.raster_bounds(font_system, &glyphs);
                result.push(super::layout::TextLayoutRun {
                    line,
                    metadata,
                    color: color.map(|color| {
                        let (r, g, b, a) = color.as_rgba_tuple();
                        [r, g, b, a]
                    }),
                    layout: TextLayout { glyphs, bounds },
                });
                start = end;
            }
        }
        result
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
            .or_insert_with(|| {
                outline_glyph_path(font_system, &mut self.rasterizer.scale_context, key)
            })
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
        self.rasterizer.clear();
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
