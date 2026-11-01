use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem, SwashContent, SwashImage};
use swash::{
    scale::{Render, ScaleContext, Source, StrikeWith},
    zeno::{Format, Placement, Vector},
};

use super::{
    options::TextSubpixelMode,
    scaler::{fake_italic_transform, with_glyph_scaler},
};

pub(super) const FREETYPE_HARMONY_LCD_SHIFT: f32 = 21.0 / 64.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(super) struct RasterGlyphKey {
    pub(super) cache_key: CacheKey,
    pub(super) subpixel_mode: TextSubpixelMode,
    pub(super) embolden_bits: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct GlyphRasterImage {
    pub(crate) content: GlyphContent,
    pub(crate) placement: Placement,
    pub(crate) data: Vec<u8>,
}

impl GlyphRasterImage {
    /// Swash is used only for alpha masks and color images. LCD coverage comes
    /// from our three-channel Harmony rasterizer, never Swash's RGBA LCD format.
    pub(super) fn from_swash(image: SwashImage) -> Self {
        let content = match image.content {
            SwashContent::Mask => GlyphContent::Mask,
            SwashContent::Color => GlyphContent::Color,
            SwashContent::SubpixelMask => unreachable!("Swash LCD rendering is not used"),
        };
        Self::new(content, image.placement, image.data)
    }

    /// Enforce the byte layout once at image creation; later stages copy the
    /// canonical data without legacy repacking or malformed-image fallbacks.
    #[allow(
        clippy::expect_used,
        reason = "Glyph raster storage must reject byte-count overflow before allocating or indexing"
    )]
    pub(super) fn new(content: GlyphContent, placement: Placement, data: Vec<u8>) -> Self {
        let channels = match content {
            GlyphContent::Mask => 1,
            GlyphContent::SubpixelMask => 3,
            GlyphContent::Color => 4,
        };
        let byte_len = (placement.width as usize)
            .checked_mul(placement.height as usize)
            .and_then(|pixels| pixels.checked_mul(channels))
            .expect("glyph image byte length overflows");
        assert_eq!(data.len(), byte_len);
        Self {
            content,
            placement,
            data,
        }
    }
}

/// Byte layouts shared by rasterization, atlas preparation and GPU upload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GlyphContent {
    Mask,
    Color,
    SubpixelMask,
}

pub(super) fn raster_glyph_image(
    font_system: &mut FontSystem,
    context: &mut ScaleContext,
    cache_key: CacheKey,
    subpixel_mode: TextSubpixelMode,
    embolden: f32,
) -> Option<GlyphRasterImage> {
    let offset = raster_glyph_offset(cache_key);
    if subpixel_mode != TextSubpixelMode::None
        && let Some(image) = raster_glyph_image_with_sources(
            font_system,
            context,
            cache_key,
            embolden,
            offset,
            &[
                Source::ColorOutline(0),
                Source::ColorBitmap(StrikeWith::BestFit),
            ],
        )
    {
        return Some(image);
    }

    if subpixel_mode != TextSubpixelMode::None {
        return raster_harmony_lcd_glyph_image(
            font_system,
            context,
            cache_key,
            subpixel_mode,
            embolden,
            offset,
        );
    }

    raster_glyph_image_with_sources(
        font_system,
        context,
        cache_key,
        embolden,
        offset,
        &[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ],
    )
}

fn raster_glyph_image_with_sources(
    font_system: &mut FontSystem,
    context: &mut ScaleContext,
    cache_key: CacheKey,
    embolden: f32,
    offset: Vector,
    sources: &[Source],
) -> Option<GlyphRasterImage> {
    with_glyph_scaler(font_system, context, cache_key, |scaler| {
        Render::new(sources)
            .format(Format::Alpha)
            .embolden(embolden.max(0.0))
            .offset(offset)
            .transform(fake_italic_transform(cache_key))
            .render(scaler, cache_key.glyph_id)
    })
    .flatten()
    .map(GlyphRasterImage::from_swash)
}

const fn raster_glyph_offset(cache_key: CacheKey) -> Vector {
    if cache_key.flags.contains(CacheKeyFlags::PIXEL_FONT) {
        Vector::new(
            cache_key.x_bin.as_float().round(),
            cache_key.y_bin.as_float().round(),
        )
    } else {
        Vector::new(cache_key.x_bin.as_float(), cache_key.y_bin.as_float())
    }
}

fn raster_harmony_lcd_glyph_image(
    font_system: &mut FontSystem,
    context: &mut ScaleContext,
    cache_key: CacheKey,
    subpixel_mode: TextSubpixelMode,
    embolden: f32,
    offset: Vector,
) -> Option<GlyphRasterImage> {
    let shifts = harmony_lcd_outline_shifts(subpixel_mode);
    with_glyph_scaler(font_system, context, cache_key, |scaler| {
        let render_channel = |scaler: &mut swash::scale::Scaler<'_>, shift: f32| {
            Render::new(&[Source::Outline])
                .format(Format::Alpha)
                .embolden(embolden.max(0.0))
                .offset(Vector::new(offset.x + shift, offset.y))
                .transform(fake_italic_transform(cache_key))
                .render(scaler, cache_key.glyph_id)
                .map(GlyphRasterImage::from_swash)
        };

        let red = render_channel(scaler, shifts[0])?;
        let green = render_channel(scaler, shifts[1])?;
        let blue = render_channel(scaler, shifts[2])?;
        // Each shifted Alpha mask already integrates pixel coverage. Harmony is
        // equivalent to a light LCD filter; filtering again spreads the edges twice.
        Some(merge_harmony_lcd_masks([red, green, blue]))
    })
    .flatten()
}

pub(super) fn harmony_lcd_outline_shifts(mode: TextSubpixelMode) -> [f32; 3] {
    match mode {
        TextSubpixelMode::None => unreachable!("Harmony LCD requires RGB or BGR"),
        // FreeType Harmony's default geometry is RGB subpixels at -21/64, 0,
        // and +21/64 px. Each channel renders the outline shifted in the
        // opposite direction. Their pixel-area integration supplies the required
        // antialiasing without a second convolution of the merged coverage.
        TextSubpixelMode::Rgb => [FREETYPE_HARMONY_LCD_SHIFT, 0.0, -FREETYPE_HARMONY_LCD_SHIFT],
        TextSubpixelMode::Bgr => [-FREETYPE_HARMONY_LCD_SHIFT, 0.0, FREETYPE_HARMONY_LCD_SHIFT],
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "Merging consumes channel buffers and releases them after constructing the combined image"
)]
pub(super) fn merge_harmony_lcd_masks(channels: [GlyphRasterImage; 3]) -> GlyphRasterImage {
    // The input always contains three channels, including empty glyph masks.
    // Their union has no missing-channel/default-bounds case.
    let [red, green, blue] = channels.each_ref().map(|image| image.placement);
    let left = red.left.min(green.left).min(blue.left);
    let right = (red.left + red.width as i32)
        .max(green.left + green.width as i32)
        .max(blue.left + blue.width as i32);
    let top = red.top.max(green.top).max(blue.top);
    let bottom = (red.top - red.height as i32)
        .min(green.top - green.height as i32)
        .min(blue.top - blue.height as i32);
    let width = (right - left).max(0) as u32;
    let height = (top - bottom).max(0) as u32;
    let mut data = vec![0; width as usize * height as usize * 3];

    for (channel, image) in channels.iter().enumerate() {
        assert_eq!(image.content, GlyphContent::Mask);
        let src_width = image.placement.width as usize;
        let src_height = image.placement.height as usize;
        let dst_x = (image.placement.left - left) as usize;
        let dst_y = (top - image.placement.top) as usize;
        for y in 0..src_height {
            for x in 0..src_width {
                let dst_ix = ((dst_y + y) * width as usize + dst_x + x) * 3 + channel;
                data[dst_ix] = image.data[y * src_width + x];
            }
        }
    }

    GlyphRasterImage::new(
        GlyphContent::SubpixelMask,
        Placement {
            left,
            top,
            width,
            height,
        },
        data,
    )
}

#[cfg(test)]
mod tests;
