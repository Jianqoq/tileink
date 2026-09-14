//! CPU packing of prepared glyph data and exact dirty buffer ranges.
use super::ranges::{changed_ranges, patch_pod_ranges};
use crate::{
    canvas::{Canvas, SceneBufferChanges},
    shared::{
        gpu_text::{GlyphImageRecord, GlyphRecord, GlyphRunRecord, text_blob_word_len},
        gpu_types::{
            GPU_GLYPH_COLOR, GPU_GLYPH_LINEAR_COLOR, GPU_GLYPH_LINEAR_MASK,
            GPU_GLYPH_LINEAR_SUBPIXEL_MASK, GPU_GLYPH_MASK, GPU_GLYPH_SUBPIXEL_MASK,
        },
        image::rgba8_pack,
        pixel::mul_div255,
    },
    text::{
        AtlasSignature, PreparedGlyphContent, PreparedTextChanges, PreparedTextData,
        TextCompositeMode,
    },
};

#[derive(Default)]
pub(crate) struct TextUpload {
    pub(crate) runs: Vec<GlyphRunRecord>,
    pub(crate) glyphs: Vec<GlyphRecord>,
    pub(crate) images: Vec<GlyphImageRecord>,
    pub(crate) image_data: Vec<u32>,
    pub(crate) atlas_signature: AtlasSignature,
    pub(crate) atlas_dirty: bool,
    pub(crate) coarse_blob: Vec<u32>,
    pub(crate) fine_blob: Vec<u32>,
    pub(crate) dirty_runs: Vec<std::ops::Range<usize>>,
    pub(crate) dirty_coarse: Vec<std::ops::Range<usize>>,
    pub(crate) dirty_fine: Vec<std::ops::Range<usize>>,
    pub(crate) fine_image_base: u32,
    pub(crate) fine_image_data_base: u32,
}

impl TextUpload {
    pub(crate) fn refill(
        &mut self,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        current_atlas_signature: AtlasSignature,
        changes: Option<&SceneBufferChanges>,
        flat_changes: Option<&PreparedTextChanges>,
    ) {
        let Some(text) = text else {
            self.clear();
            return;
        };
        self.dirty_runs.clear();
        self.dirty_coarse.clear();
        self.dirty_fine.clear();
        let old_run_len = self.runs.len();
        let old_glyph_len = self.glyphs.len();
        let incremental = changes.is_some() || flat_changes.is_some();
        let changed_runs = changes
            .map(|changes| changes.text_runs.as_slice())
            .or_else(|| flat_changes.map(PreparedTextChanges::runs));
        let changed_glyphs = changes
            .map(|changes| changes.glyphs.as_slice())
            .or_else(|| flat_changes.map(PreparedTextChanges::glyphs));
        self.runs
            .resize(canvas.text_runs.len(), GlyphRunRecord::default());
        self.glyphs
            .resize(canvas.text_glyphs.len(), GlyphRecord::default());
        let run_ranges = changed_ranges(changed_runs, old_run_len, self.runs.len());
        let glyph_ranges = changed_ranges(changed_glyphs, old_glyph_len, self.glyphs.len());
        for range in &run_ranges {
            for index in range.clone() {
                let run = canvas.text_runs[index];
                self.runs[index] = GlyphRunRecord {
                    glyph_start: run.glyph_start,
                    glyph_count: run.glyph_count,
                };
            }
        }
        for range in &glyph_ranges {
            for index in range.clone() {
                let glyph = canvas.text_glyphs[index];
                self.glyphs[index] = GlyphRecord {
                    image_id: text
                        .image_id_for_cache_key(glyph.cache_key)
                        .unwrap_or(u32::MAX),
                    x: glyph.x,
                    y: glyph.y,
                };
            }
        }
        self.dirty_runs.extend(run_ranges.iter().cloned());
        let atlas_signature = text.atlas_signature();
        self.atlas_dirty = atlas_signature != current_atlas_signature;
        self.atlas_signature = atlas_signature;
        if self.atlas_dirty {
            self.rebuild_images(text);
        }

        let layout_changed = !incremental
            || old_run_len != self.runs.len()
            || old_glyph_len != self.glyphs.len()
            || self.atlas_dirty;
        if layout_changed {
            self.rebuild_blobs();
        } else {
            let run_words = std::mem::size_of::<GlyphRunRecord>() / 4;
            let glyph_words = std::mem::size_of::<GlyphRecord>() / 4;
            let coarse_glyph_base = self.runs.len() * run_words;
            patch_pod_ranges(
                &mut self.coarse_blob,
                0,
                &self.runs,
                &run_ranges,
                &mut self.dirty_coarse,
            );
            patch_pod_ranges(
                &mut self.coarse_blob,
                coarse_glyph_base,
                &self.glyphs,
                &glyph_ranges,
                &mut self.dirty_coarse,
            );
            patch_pod_ranges(
                &mut self.fine_blob,
                0,
                &self.glyphs,
                &glyph_ranges,
                &mut self.dirty_fine,
            );
            debug_assert_eq!(glyph_words, 3);
        }
    }

    fn rebuild_images(&mut self, text: &PreparedTextData) {
        self.images.clear();
        self.image_data.clear();
        for image in text.images() {
            let data_offset = self.image_data.len() as u32;
            let content = match image.content {
                PreparedGlyphContent::Mask => {
                    self.image_data
                        .extend(image.data.iter().map(|&alpha| alpha as u32));
                    match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_MASK,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_MASK,
                    }
                }
                PreparedGlyphContent::Color => {
                    for pixel in image.data.as_chunks::<4>().0 {
                        let a = pixel[3];
                        self.image_data.push(rgba8_pack([
                            mul_div255(pixel[0], a),
                            mul_div255(pixel[1], a),
                            mul_div255(pixel[2], a),
                            a,
                        ]));
                    }
                    match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_COLOR,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_COLOR,
                    }
                }
                PreparedGlyphContent::SubpixelMask => {
                    for pixel in image.data.as_chunks::<3>().0 {
                        self.image_data
                            .push(rgba8_pack([pixel[0], pixel[1], pixel[2], 0]));
                    }
                    match image.composite_mode {
                        TextCompositeMode::Srgb => GPU_GLYPH_SUBPIXEL_MASK,
                        TextCompositeMode::Linear => GPU_GLYPH_LINEAR_SUBPIXEL_MASK,
                    }
                }
            };
            self.images.push(GlyphImageRecord {
                left: image.left,
                top: image.top,
                width: image.width,
                height: image.height,
                content,
                data_offset,
            });
        }
    }

    fn rebuild_blobs(&mut self) {
        self.coarse_blob.clear();
        self.coarse_blob.reserve(text_blob_word_len(
            self.runs.len(),
            self.glyphs.len(),
            self.images.len(),
        ));
        self.coarse_blob
            .extend_from_slice(bytemuck::cast_slice(&self.runs));
        self.coarse_blob
            .extend_from_slice(bytemuck::cast_slice(&self.glyphs));
        self.coarse_blob
            .extend_from_slice(bytemuck::cast_slice(&self.images));
        self.dirty_coarse.push(0..self.coarse_blob.len());

        self.fine_image_base = (self.glyphs.len() * std::mem::size_of::<GlyphRecord>() / 4) as u32;
        self.fine_image_data_base = self.fine_image_base
            + (self.images.len() * std::mem::size_of::<GlyphImageRecord>() / 4) as u32;
        self.fine_blob.clear();
        self.fine_blob
            .reserve(self.fine_image_data_base as usize + self.image_data.len());
        self.fine_blob
            .extend_from_slice(bytemuck::cast_slice(&self.glyphs));
        self.fine_blob
            .extend_from_slice(bytemuck::cast_slice(&self.images));
        self.fine_blob.extend_from_slice(&self.image_data);
        self.dirty_fine.push(0..self.fine_blob.len());
    }

    fn clear(&mut self) {
        self.runs.clear();
        self.glyphs.clear();
        self.images.clear();
        self.image_data.clear();
        self.atlas_signature = AtlasSignature::default();
        self.atlas_dirty = false;
        self.coarse_blob.clear();
        self.fine_blob.clear();
        self.dirty_runs.clear();
        self.dirty_coarse.clear();
        self.dirty_fine.clear();
        self.fine_image_base = 0;
        self.fine_image_data_base = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::super::glyph_capacity::tests::glyph_capacity_fixture;
    use super::*;

    #[test]
    fn flat_text_changes_patch_only_changed_gpu_glyph_records() {
        let (_, mut canvas, text) = glyph_capacity_fixture();
        let mut upload = TextUpload::default();
        upload.refill(&canvas, Some(&text), AtlasSignature::default(), None, None);
        canvas.text_glyphs[1].x += 5;
        let changes = PreparedTextChanges::from_ranges(std::iter::once(1..2).collect(), Vec::new());

        upload.refill(
            &canvas,
            Some(&text),
            text.atlas_signature(),
            None,
            Some(&changes),
        );

        assert!(upload.dirty_runs.is_empty());
        assert!(!upload.dirty_coarse.is_empty());
        assert!(
            upload
                .dirty_coarse
                .iter()
                .all(|range| range.len() < upload.coarse_blob.len())
        );
        assert!(
            upload
                .dirty_fine
                .iter()
                .all(|range| range.len() < upload.fine_blob.len())
        );
    }
}
