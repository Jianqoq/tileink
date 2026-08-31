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
    pub(crate) content: SwashContent,
    pub(crate) placement: Placement,
    pub(crate) data: Vec<u8>,
}

impl GlyphRasterImage {
    pub(super) fn from_swash(image: SwashImage) -> Self {
        Self {
            content: image.content,
            placement: image.placement,
            data: image.data,
        }
    }

    #[cfg(test)]
    pub(crate) fn subpixel_mask(placement: Placement, data: Vec<u8>) -> Self {
        Self {
            content: SwashContent::SubpixelMask,
            placement,
            data,
        }
    }
}

pub(super) fn raster_glyph_image(
    font_system: &mut FontSystem,
    context: &mut ScaleContext,
    cache_key: CacheKey,
    subpixel_mode: TextSubpixelMode,
    embolden: f32,
) -> Option<SwashImage> {
    let offset = raster_glyph_offset(cache_key);
    if subpixel_mode != TextSubpixelMode::None
        && let Some(image) = raster_glyph_image_with_sources(
            font_system,
            context,
            cache_key,
            Format::Alpha,
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
        Format::Alpha,
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
    format: Format,
    embolden: f32,
    offset: Vector,
    sources: &[Source],
) -> Option<SwashImage> {
    with_glyph_scaler(font_system, context, cache_key, |scaler| {
        Render::new(sources)
            .format(format)
            .embolden(embolden.max(0.0))
            .offset(offset)
            .transform(fake_italic_transform(cache_key))
            .render(scaler, cache_key.glyph_id)
    })
    .flatten()
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
) -> Option<SwashImage> {
    let shifts = harmony_lcd_outline_shifts(subpixel_mode);
    with_glyph_scaler(font_system, context, cache_key, |scaler| {
        let render_channel = |scaler: &mut swash::scale::Scaler<'_>, shift: f32| {
            Render::new(&[Source::Outline])
                .format(Format::Alpha)
                .embolden(embolden.max(0.0))
                .offset(Vector::new(offset.x + shift, offset.y))
                .transform(fake_italic_transform(cache_key))
                .render(scaler, cache_key.glyph_id)
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
        TextSubpixelMode::None => [0.0; 3],
        // FreeType Harmony's default geometry is RGB subpixels at -21/64, 0,
        // and +21/64 px. Each channel renders the outline shifted in the
        // opposite direction; the light FIR pass below only damps residual
        // color fringes rather than defining the LCD sampling geometry.
        TextSubpixelMode::Rgb => [FREETYPE_HARMONY_LCD_SHIFT, 0.0, -FREETYPE_HARMONY_LCD_SHIFT],
        TextSubpixelMode::Bgr => [-FREETYPE_HARMONY_LCD_SHIFT, 0.0, FREETYPE_HARMONY_LCD_SHIFT],
    }
}

pub(super) fn merge_harmony_lcd_masks(channels: [SwashImage; 3]) -> SwashImage {
    let left = channels
        .iter()
        .map(|image| image.placement.left)
        .min()
        .unwrap_or(0);
    let right = channels
        .iter()
        .map(|image| image.placement.left + image.placement.width as i32)
        .max()
        .unwrap_or(left);
    let top = channels
        .iter()
        .map(|image| image.placement.top)
        .max()
        .unwrap_or(0);
    let bottom = channels
        .iter()
        .map(|image| image.placement.top - image.placement.height as i32)
        .min()
        .unwrap_or(top);
    let width = (right - left).max(0) as u32;
    let height = (top - bottom).max(0) as u32;
    let mut data = vec![0; width as usize * height as usize * 3];

    for (channel, image) in channels.iter().enumerate() {
        if image.content != SwashContent::Mask {
            continue;
        }
        let src_width = image.placement.width as usize;
        let src_height = image.placement.height as usize;
        if image.data.len() != src_width * src_height {
            continue;
        }
        let dst_x = (image.placement.left - left) as usize;
        let dst_y = (top - image.placement.top) as usize;
        for y in 0..src_height {
            for x in 0..src_width {
                let dst_ix = ((dst_y + y) * width as usize + dst_x + x) * 3 + channel;
                data[dst_ix] = image.data[y * src_width + x];
            }
        }
    }

    let mut image = SwashImage::new();
    image.content = SwashContent::SubpixelMask;
    image.source = Source::Outline;
    image.placement = Placement {
        left,
        top,
        width,
        height,
    };
    image.data = data;
    image
}

/// Applies a center-weighted LCD FIR filter to the interleaved RGB coverage
/// samples. One transparent pixel of horizontal padding preserves both filter
/// tails instead of clipping them at the glyph bitmap boundary.
pub(super) fn filter_harmony_lcd_mask(image: &mut SwashImage, mode: TextSubpixelMode) {
    if image.content != SwashContent::SubpixelMask
        || mode == TextSubpixelMode::None
        || image.placement.width == 0
        || image.placement.height == 0
    {
        return;
    }

    let input_width = image.placement.width as usize * 3;
    if image.data.len() != input_width * image.placement.height as usize {
        return;
    }

    let output_width = input_width + 6;
    let mut filtered = vec![0; output_width * image.placement.height as usize];
    let mut accumulated = vec![0_u32; output_width];
    let physical_to_memory = match mode {
        TextSubpixelMode::Rgb => [0, 1, 2],
        TextSubpixelMode::Bgr => [2, 1, 0],
        TextSubpixelMode::None => unreachable!(),
    };
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
