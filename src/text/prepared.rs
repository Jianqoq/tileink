use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

use cosmic_text::{CacheKey, FontSystem};

use crate::shared::bounds::Bounds;

use super::{
    context::TextContext,
    layout::{CanvasGlyph, TextRun, glyph_image_bounds},
    options::{TextCompositeMode, TextRasterOptions},
    raster::{GlyphContent, GlyphRasterImage},
};

mod reconcile;
pub(crate) use reconcile::PreparedTextChanges;

#[derive(Clone, Debug)]
pub(crate) struct PreparedTextData {
    runs: Vec<TextRun>,
    glyphs: Vec<PreparedGlyph>,
    images: Vec<PreparedGlyphImage>,
    image_by_key: HashMap<CacheKey, u32>,
    atlas_signature: AtlasSignature,
    raster_options: TextRasterOptions,
    cache_generation: u64,
    image_bytes: usize,
}

impl PreparedTextData {
    pub(crate) fn new(
        glyphs: &[CanvasGlyph],
        runs: &[TextRun],
        font_system: &mut FontSystem,
        context: &mut TextContext,
    ) -> Self {
        let mut image_by_key = HashMap::new();
        let mut images = Vec::new();
        let mut prepared_glyphs = Vec::with_capacity(glyphs.len());
        let mut atlas_hasher = StableAtlasHasher::new();
        let mut atlas_len = 0u32;
        let mut image_bytes = 0;
        let raster_options = context.raster_options();
        let cache_generation = context.cache_generation();

        for glyph in glyphs {
            let image = if let Some(&image) = image_by_key.get(&glyph.cache_key) {
                Some(image)
            } else if let Some(image) = context.glyph_image(font_system, glyph.cache_key) {
                let image_ix = images.len() as u32;
                let image = PreparedGlyphImage::from_raster(image, raster_options);
                image_bytes += image.data.len();
                images.push(image);
                image_by_key.insert(glyph.cache_key, image_ix);
                glyph.cache_key.hash(&mut atlas_hasher);
                raster_options.hash(&mut atlas_hasher);
                atlas_len += 1;
                Some(image_ix)
            } else {
                None
            };
            prepared_glyphs.push(PreparedGlyph {
                cache_key: glyph.cache_key,
                image,
                x: glyph.x,
                y: glyph.y,
            });
        }
        atlas_hasher.write_u64(cache_generation);

        Self {
            runs: runs.to_vec(),
            glyphs: prepared_glyphs,
            images,
            image_by_key,
            atlas_signature: AtlasSignature::from_hash(atlas_len, atlas_hasher.finish128()),
            raster_options,
            cache_generation,
            image_bytes,
        }
    }

    fn rebuild_atlas_signature(&mut self) {
        let mut hashes = self
            .image_by_key
            .keys()
            .map(|key| {
                let mut hasher = StableAtlasHasher::new();
                key.hash(&mut hasher);
                self.raster_options.hash(&mut hasher);
                hasher.finish()
            })
            .collect::<Vec<_>>();
        hashes.sort_unstable();
        let mut hasher = StableAtlasHasher::new();
        for hash in hashes {
            hasher.write_u64(hash);
        }
        hasher.write_u64(self.cache_generation);
        self.atlas_signature =
            AtlasSignature::from_hash(self.image_by_key.len() as u32, hasher.finish128());
    }

    pub(crate) fn run_glyph_indices(&self, run_id: u32) -> std::ops::Range<u32> {
        let Some(run) = self.runs.get(run_id as usize) else {
            return 0..0;
        };
        run.glyph_start..run.glyph_start.saturating_add(run.glyph_count)
    }

    pub(crate) fn glyph(&self, glyph_id: u32) -> Option<&PreparedGlyph> {
        self.glyphs.get(glyph_id as usize)
    }

    pub(crate) fn glyph_bounds(&self, glyph_id: u32) -> Option<Bounds> {
        let glyph = self.glyph(glyph_id)?;
        let image = self.image(glyph.image?)?;
        Some(glyph_image_bounds(
            glyph.x,
            glyph.y,
            image.left,
            image.top,
            image.width,
            image.height,
        ))
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

    pub(crate) fn atlas_signature(&self) -> AtlasSignature {
        self.atlas_signature
    }

    #[cfg(test)]
    pub(super) fn cache_generation(&self) -> u64 {
        self.cache_generation
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AtlasSignature {
    len: u32,
    hash0: u64,
    hash1: u64,
}

impl AtlasSignature {
    fn from_hash(len: u32, hash: (u64, u64)) -> Self {
        if len == 0 {
            Self::default()
        } else {
            Self {
                len,
                hash0: hash.0,
                hash1: hash.1,
            }
        }
    }
}

struct StableAtlasHasher {
    a: u64,
    b: u64,
}

impl StableAtlasHasher {
    fn new() -> Self {
        Self {
            a: 0xcbf2_9ce4_8422_2325,
            b: 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn finish128(self) -> (u64, u64) {
        (avalanche64(self.a), avalanche64(self.b))
    }

    fn mix_byte(&mut self, byte: u8) {
        self.a ^= byte as u64;
        self.a = self.a.wrapping_mul(0x0000_0100_0000_01b3);
        self.b ^= (byte as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.b = self.b.rotate_left(27).wrapping_mul(0x3c79_ac49_2ba7_b653);
    }
}

impl Hasher for StableAtlasHasher {
    fn finish(&self) -> u64 {
        avalanche64(self.a ^ self.b)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.mix_byte(byte);
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.write(&i.to_le_bytes());
    }

    fn write_u16(&mut self, i: u16) {
        self.write(&i.to_le_bytes());
    }

    fn write_u32(&mut self, i: u32) {
        self.write(&i.to_le_bytes());
    }

    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }

    fn write_usize(&mut self, i: usize) {
        self.write(&(i as u64).to_le_bytes());
    }

    fn write_i8(&mut self, i: i8) {
        self.write_u8(i as u8);
    }

    fn write_i16(&mut self, i: i16) {
        self.write_u16(i as u16);
    }

    fn write_i32(&mut self, i: i32) {
        self.write_u32(i as u32);
    }

    fn write_i64(&mut self, i: i64) {
        self.write_u64(i as u64);
    }

    fn write_isize(&mut self, i: isize) {
        self.write_usize(i as usize);
    }
}

fn avalanche64(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    value ^ (value >> 33)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedGlyph {
    pub(crate) cache_key: CacheKey,
    pub(crate) image: Option<u32>,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedGlyphImage {
    pub(crate) content: GlyphContent,
    pub(crate) composite_mode: TextCompositeMode,
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data: Vec<u8>,
}

impl PreparedGlyphImage {
    pub(super) fn from_raster(image: &GlyphRasterImage, raster_options: TextRasterOptions) -> Self {
        Self {
            content: image.content,
            composite_mode: raster_options.composite_mode,
            left: image.placement.left,
            top: image.placement.top,
            width: image.placement.width,
            height: image.placement.height,
            data: image.data.clone(),
        }
    }
}
