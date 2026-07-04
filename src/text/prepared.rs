use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

#[cfg(test)]
use cosmic_text::SwashImage;
use cosmic_text::{CacheKey, SwashContent};

use crate::shared::{bounds::Bounds, pixel::TextCoverageParams};

use super::{
    context::TextContext,
    layout::{CanvasGlyph, TextRun},
    options::{TextCompositeMode, TextRasterOptions},
    raster::GlyphRasterImage,
};

#[derive(Clone, Debug)]
pub(crate) struct PreparedTextData {
    runs: Vec<TextRun>,
    glyphs: Vec<PreparedGlyph>,
    images: Vec<PreparedGlyphImage>,
    image_by_key: HashMap<CacheKey, u32>,
    atlas_signature: AtlasSignature,
}

impl PreparedTextData {
    pub(crate) fn new(glyphs: &[CanvasGlyph], runs: &[TextRun], context: &mut TextContext) -> Self {
        let mut image_by_key = HashMap::new();
        let mut images = Vec::new();
        let mut prepared_glyphs = Vec::with_capacity(glyphs.len());
        let mut atlas_hasher = StableAtlasHasher::new();
        let mut atlas_len = 0u32;
        let raster_options = context.raster_options();

        for glyph in glyphs {
            let image = if let Some(&image) = image_by_key.get(&glyph.cache_key) {
                Some(image)
            } else if let Some(image) = context.glyph_image(glyph.cache_key) {
                let image_ix = images.len() as u32;
                images.push(PreparedGlyphImage::from_raster(image, raster_options));
                image_by_key.insert(glyph.cache_key, image_ix);
                glyph.cache_key.hash(&mut atlas_hasher);
                raster_options.hash(&mut atlas_hasher);
                atlas_len += 1;
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
            atlas_signature: AtlasSignature::from_hash(atlas_len, atlas_hasher.finish128()),
        }
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
        Some(glyph.bounds(image))
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
    pub(crate) fn from_test_parts(
        glyphs: Vec<PreparedGlyph>,
        runs: Vec<TextRun>,
        images: Vec<PreparedGlyphImage>,
    ) -> Self {
        Self {
            runs,
            glyphs,
            images,
            image_by_key: HashMap::new(),
            atlas_signature: AtlasSignature::default(),
        }
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
    pub(crate) image: Option<u32>,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl PreparedGlyph {
    fn bounds(&self, image: &PreparedGlyphImage) -> Bounds {
        let x0 = self.x + image.left;
        let y0 = self.y - image.top;
        Bounds::new(x0, y0, x0 + image.width as i32, y0 + image.height as i32)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedGlyphImage {
    pub(crate) content: PreparedGlyphContent,
    pub(crate) composite_mode: TextCompositeMode,
    pub(crate) coverage_params: TextCoverageParams,
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data: Vec<u8>,
}

impl PreparedGlyphImage {
    pub(super) fn from_raster(image: &GlyphRasterImage, raster_options: TextRasterOptions) -> Self {
        let content = match image.content {
            SwashContent::Mask => PreparedGlyphContent::Mask,
            SwashContent::Color => PreparedGlyphContent::Color,
            SwashContent::SubpixelMask => PreparedGlyphContent::SubpixelMask,
        };
        let pixels = prepared_glyph_image_data(content, image);
        Self {
            content,
            composite_mode: raster_options.composite_mode,
            coverage_params: raster_options.coverage_params,
            left: pixels.left,
            top: image.placement.top,
            width: pixels.width,
            height: image.placement.height,
            data: pixels.data,
        }
    }

    #[cfg(test)]
    pub(super) fn from_swash(image: &SwashImage, raster_options: TextRasterOptions) -> Self {
        Self::from_raster(&GlyphRasterImage::from_swash(image.clone()), raster_options)
    }
}

struct PreparedGlyphPixels {
    left: i32,
    width: u32,
    data: Vec<u8>,
}

fn prepared_glyph_image_data(
    content: PreparedGlyphContent,
    image: &GlyphRasterImage,
) -> PreparedGlyphPixels {
    if content != PreparedGlyphContent::SubpixelMask {
        return PreparedGlyphPixels {
            left: image.placement.left,
            width: image.placement.width,
            data: image.data.clone(),
        };
    }

    let pixel_count = image.placement.width as usize * image.placement.height as usize;
    let data = if image.data.len() == pixel_count * 4 {
        let mut data = Vec::with_capacity(pixel_count * 3);
        for pixel in image.data.chunks_exact(4) {
            data.extend_from_slice(&pixel[..3]);
        }
        data
    } else if image.data.len() == pixel_count * 3 {
        image.data.clone()
    } else {
        return PreparedGlyphPixels {
            left: image.placement.left,
            width: image.placement.width,
            data: image.data.clone(),
        };
    };

    if image.placement.width == 0 || image.placement.height == 0 {
        return PreparedGlyphPixels {
            left: image.placement.left,
            width: image.placement.width,
            data,
        };
    }

    PreparedGlyphPixels {
        left: image.placement.left,
        width: image.placement.width,
        data,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreparedGlyphContent {
    Mask,
    Color,
    SubpixelMask,
}
