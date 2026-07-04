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
        Some(merge_harmony_lcd_masks([red, green, blue]))
    })
    .flatten()
}

pub(super) fn harmony_lcd_outline_shifts(mode: TextSubpixelMode) -> [f32; 3] {
    match mode {
        TextSubpixelMode::None => [0.0; 3],
        // FreeType Harmony's default geometry is RGB subpixels at -21/64, 0,
        // and +21/64 px. Each channel renders the outline shifted in the
        // opposite direction so channel coverages stay integral and do not
        // need ClearType-style FIR filtering.
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
