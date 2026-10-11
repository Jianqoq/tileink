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
const LCD_FILTER_SIDE_WEIGHT: u32 = 21;
const LCD_FILTER_CENTER_WEIGHT: u32 = 214;
const LCD_FILTER_DIVISOR: u32 = 256;

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

fn raster_glyph_offset(cache_key: CacheKey) -> Vector {
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
        let mut image = merge_harmony_lcd_masks([red, green, blue]);
        filter_harmony_lcd_mask(&mut image, subpixel_mode);
        Some(image)
    })
    .flatten()
}

pub(super) fn harmony_lcd_outline_shifts(mode: TextSubpixelMode) -> [f32; 3] {
    match mode {
        TextSubpixelMode::None => unreachable!("Harmony LCD requires RGB or BGR"),
        // FreeType Harmony's default geometry is RGB subpixels at -21/64, 0,
        // and +21/64 px. Each channel renders the outline shifted in the
        // opposite direction; the light FIR pass below only damps residual
        // color fringes rather than defining the LCD sampling geometry.
        TextSubpixelMode::Rgb => [FREETYPE_HARMONY_LCD_SHIFT, 0.0, -FREETYPE_HARMONY_LCD_SHIFT],
        TextSubpixelMode::Bgr => [-FREETYPE_HARMONY_LCD_SHIFT, 0.0, FREETYPE_HARMONY_LCD_SHIFT],
    }
}

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

/// Applies a center-weighted LCD FIR filter to the interleaved RGB coverage
/// samples. One transparent pixel of horizontal padding preserves both filter
/// tails instead of clipping them at the glyph bitmap boundary.
pub(super) fn filter_harmony_lcd_mask(image: &mut GlyphRasterImage, mode: TextSubpixelMode) {
    assert_eq!(image.content, GlyphContent::SubpixelMask);
    let physical_to_memory = match mode {
        TextSubpixelMode::Rgb => [0, 1, 2],
        TextSubpixelMode::Bgr => [2, 1, 0],
        TextSubpixelMode::None => unreachable!("Harmony LCD requires RGB or BGR"),
    };
    if image.placement.width == 0 || image.placement.height == 0 {
        return;
    }

    let input_width = image.placement.width as usize * 3;

    let output_width = input_width + 6;
    let mut filtered = vec![0; output_width * image.placement.height as usize];
    let mut accumulated = vec![0_u32; output_width];
    for row in 0..image.placement.height as usize {
        let input = &image.data[row * input_width..(row + 1) * input_width];
        let output = &mut filtered[row * output_width..(row + 1) * output_width];
        accumulated.fill(0);
        for pixel in 0..image.placement.width as usize {
            for (physical_channel, &memory_channel) in physical_to_memory.iter().enumerate() {
                let sample = u32::from(input[pixel * 3 + memory_channel]);
                let sample_index = pixel * 3 + physical_channel;
                accumulated[sample_index + 2] += LCD_FILTER_SIDE_WEIGHT * sample;
                accumulated[sample_index + 3] += LCD_FILTER_CENTER_WEIGHT * sample;
                accumulated[sample_index + 4] += LCD_FILTER_SIDE_WEIGHT * sample;
            }
        }
        for pixel in 0..image.placement.width as usize + 2 {
            for (physical_channel, &memory_channel) in physical_to_memory.iter().enumerate() {
                output[pixel * 3 + memory_channel] =
                    ((accumulated[pixel * 3 + physical_channel] + LCD_FILTER_DIVISOR / 2)
                        / LCD_FILTER_DIVISOR) as u8;
            }
        }
    }

    image.placement.left -= 1;
    image.placement.width += 2;
    image.data = filtered;
}
