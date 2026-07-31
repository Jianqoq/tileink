use std::ops::Range;

use cosmic_text::FontSystem;

use super::{
    CanvasGlyph, PreparedGlyph, PreparedGlyphImage, PreparedTextData, TextContext, TextRun,
};

const MAX_RETAINED_GLYPH_IMAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedTextChanges {
    glyphs: Vec<Range<usize>>,
    runs: Vec<Range<usize>>,
}

impl PreparedTextChanges {
    pub(crate) fn glyphs(&self) -> &[Range<usize>] {
        &self.glyphs
    }

    pub(crate) fn runs(&self) -> &[Range<usize>] {
        &self.runs
    }

    #[cfg(test)]
    pub(crate) fn from_ranges(glyphs: Vec<Range<usize>>, runs: Vec<Range<usize>>) -> Self {
        Self { glyphs, runs }
    }
}

impl PreparedTextData {
    /// Applies retained arena edits without rerasterizing or walking unchanged glyphs.
    pub(crate) fn update(
        &mut self,
        glyphs: &[CanvasGlyph],
        runs: &[TextRun],
        glyph_ranges: &[Range<usize>],
        run_ranges: &[Range<usize>],
        font_system: &mut FontSystem,
        context: &mut TextContext,
    ) {
        if self.requires_rebuild(context) {
            *self = Self::new(glyphs, runs, font_system, context);
            return;
        }

        let old_glyph_len = self.glyphs.len();
        let mut changed_glyphs = glyph_ranges.to_vec();
        if glyphs.len() > old_glyph_len {
            changed_glyphs.push(old_glyph_len..glyphs.len());
        }
        merge_ranges(&mut changed_glyphs);
        self.glyphs.truncate(glyphs.len());
        let mut atlas_changed = false;
        for range in changed_glyphs {
            let range = range.start.min(glyphs.len())..range.end.min(glyphs.len());
            for index in range {
                let prepared =
                    self.prepare_glyph(glyphs[index], font_system, context, &mut atlas_changed);
                replace_or_push(&mut self.glyphs, index, prepared);
            }
        }

        let old_run_len = self.runs.len();
        self.runs.resize(
            runs.len(),
            TextRun {
                glyph_start: 0,
                glyph_count: 0,
            },
        );
        let mut changed_runs = run_ranges.to_vec();
        if runs.len() > old_run_len {
            changed_runs.push(old_run_len..runs.len());
        }
        merge_ranges(&mut changed_runs);
        for range in changed_runs {
            let range = range.start.min(runs.len())..range.end.min(runs.len());
            self.runs[range.clone()].copy_from_slice(&runs[range]);
        }
        self.finish_update(
            glyphs,
            runs,
            atlas_changed,
            font_system,
            context,
            MAX_RETAINED_GLYPH_IMAGE_BYTES,
        );
    }

    /// Reconciles a newly recorded flat Canvas against the previous prepared text.
    ///
    /// Immediate responsive frames do not carry retained arena ranges. Scanning their compact
    /// glyph/run arrays is still cheaper than cloning every cached bitmap and rebuilding the atlas
    /// lookup, and position-only changes reuse the existing image id without a hash lookup.
    pub(crate) fn reconcile(
        &mut self,
        glyphs: &[CanvasGlyph],
        runs: &[TextRun],
        font_system: &mut FontSystem,
        context: &mut TextContext,
    ) -> Option<PreparedTextChanges> {
        self.reconcile_with_image_budget(
            glyphs,
            runs,
            font_system,
            context,
            MAX_RETAINED_GLYPH_IMAGE_BYTES,
        )
    }

    pub(crate) fn reconcile_with_image_budget(
        &mut self,
        glyphs: &[CanvasGlyph],
        runs: &[TextRun],
        font_system: &mut FontSystem,
        context: &mut TextContext,
        max_image_bytes: usize,
    ) -> Option<PreparedTextChanges> {
        if self.requires_rebuild(context) {
            *self = Self::new(glyphs, runs, font_system, context);
            return None;
        }

        self.glyphs.truncate(glyphs.len());
        let mut changes = PreparedTextChanges::default();
        let mut atlas_changed = false;
        for (index, &glyph) in glyphs.iter().enumerate() {
            if let Some(prepared) = self.glyphs.get_mut(index)
                && prepared.cache_key == glyph.cache_key
            {
                if (prepared.x, prepared.y) == (glyph.x, glyph.y) {
                    continue;
                }
                prepared.x = glyph.x;
                prepared.y = glyph.y;
                push_index(&mut changes.glyphs, index);
                continue;
            }
            let prepared = self.prepare_glyph(glyph, font_system, context, &mut atlas_changed);
            replace_or_push(&mut self.glyphs, index, prepared);
            push_index(&mut changes.glyphs, index);
        }

        self.runs.truncate(runs.len());
        for (index, &run) in runs.iter().enumerate() {
            if self.runs.get(index) == Some(&run) {
                continue;
            }
            replace_or_push(&mut self.runs, index, run);
            push_index(&mut changes.runs, index);
        }
        if self.finish_update(
            glyphs,
            runs,
            atlas_changed,
            font_system,
            context,
            max_image_bytes,
        ) {
            None
        } else {
            Some(changes)
        }
    }

    fn requires_rebuild(&self, context: &TextContext) -> bool {
        context.raster_options() != self.raster_options
            || context.cache_generation() != self.cache_generation
    }

    fn prepare_glyph(
        &mut self,
        glyph: CanvasGlyph,
        font_system: &mut FontSystem,
        context: &mut TextContext,
        atlas_changed: &mut bool,
    ) -> PreparedGlyph {
        let image = if let Some(&image) = self.image_by_key.get(&glyph.cache_key) {
            Some(image)
        } else if let Some(image) = context.glyph_image(font_system, glyph.cache_key) {
            let image_id = self.images.len() as u32;
            let image = PreparedGlyphImage::from_raster(image, self.raster_options);
            self.image_bytes += image.data.len();
            self.images.push(image);
            self.image_by_key.insert(glyph.cache_key, image_id);
            *atlas_changed = true;
            Some(image_id)
        } else {
            None
        };
        PreparedGlyph {
            cache_key: glyph.cache_key,
            image,
            x: glyph.x,
            y: glyph.y,
        }
    }

    fn finish_update(
        &mut self,
        glyphs: &[CanvasGlyph],
        runs: &[TextRun],
        atlas_changed: bool,
        font_system: &mut FontSystem,
        context: &mut TextContext,
        max_image_bytes: usize,
    ) -> bool {
        if !atlas_changed {
            return false;
        }
        if self.image_bytes > max_image_bytes {
            // Root-cause memory bound: a long-lived editor can encounter an unbounded sequence of
            // glyph keys. Rebuild from the live frame after the configured image-data budget.
            *self = Self::new(glyphs, runs, font_system, context);
            true
        } else {
            self.rebuild_atlas_signature();
            false
        }
    }
}

fn replace_or_push<T>(values: &mut Vec<T>, index: usize, value: T) {
    if index < values.len() {
        values[index] = value;
    } else {
        debug_assert_eq!(index, values.len());
        values.push(value);
    }
}

fn push_index(ranges: &mut Vec<Range<usize>>, index: usize) {
    if let Some(last) = ranges.last_mut()
        && last.end == index
    {
        last.end += 1;
    } else {
        ranges.push(index..index + 1);
    }
}

fn merge_ranges(ranges: &mut Vec<Range<usize>>) {
    ranges.retain(|range| !range.is_empty());
    ranges.sort_unstable_by_key(|range| range.start);
    let mut merged = Vec::<Range<usize>>::with_capacity(ranges.len());
    for range in ranges.drain(..) {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    *ranges = merged;
}
