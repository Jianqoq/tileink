use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

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
    atlas_signature: AtlasSignature,
}

impl PreparedTextData {
    pub(crate) fn new(glyphs: &[SceneGlyph], runs: &[TextRun], context: &mut TextContext) -> Self {
        let mut image_by_key = HashMap::new();
        let mut images = Vec::new();
        let mut prepared_glyphs = Vec::with_capacity(glyphs.len());
        let mut atlas_hasher = StableAtlasHasher::new();
        let mut atlas_len = 0u32;

        for glyph in glyphs {
            let image = if let Some(&image) = image_by_key.get(&glyph.cache_key) {
                Some(image)
            } else if let Some(image) = context.glyph_image(glyph.cache_key) {
                let image_ix = images.len() as u32;
                images.push(PreparedGlyphImage::from_swash(image));
                image_by_key.insert(glyph.cache_key, image_ix);
                glyph.cache_key.hash(&mut atlas_hasher);
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

    #[test]
    fn prepared_text_signature_changes_with_glyph_images() {
        let mut context = TextContext::new();
        let a = context.layout(TextLayoutOptions::new("A", 20.0));
        let b = context.layout(TextLayoutOptions::new("B", 20.0));
        if a.is_empty() || b.is_empty() {
            return;
        }

        let a_glyphs: Vec<_> = scene_glyphs_at_origin(&a, Point::new(0.0, 0.0)).collect();
        let b_glyphs: Vec<_> = scene_glyphs_at_origin(&b, Point::new(0.0, 0.0)).collect();
        let runs = [TextRun {
            glyph_start: 0,
            glyph_count: 1,
        }];
        let a_data = PreparedTextData::new(&a_glyphs, &runs, &mut context);
        let b_data = PreparedTextData::new(&b_glyphs, &runs, &mut context);

        assert_ne!(a_data.atlas_signature(), b_data.atlas_signature());
    }
}
