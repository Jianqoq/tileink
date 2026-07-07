use std::path::Path;

use cosmic_text::{
    Align, Attrs, CacheKeyFlags, Family, FontSystem, SwashContent, SwashImage, Weight,
};
use peniko::{
    Color,
    kurbo::{Affine, Point, Shape},
};
use swash::zeno::Placement;

use crate::{Canvas, shared::draw_record::DrawTag};

use super::{
    raster::{
        FREETYPE_HARMONY_LCD_SHIFT, GlyphRasterImage, harmony_lcd_outline_shifts,
        merge_harmony_lcd_masks,
    },
    *,
};

#[test]
fn layout_produces_positioned_glyphs_when_a_font_is_available() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("Hello", 24.0));
    if layout.glyphs.is_empty() {
        return;
    }

    assert!(!layout.bounds().is_empty());
}

#[test]
fn load_font_file_makes_font_available_to_layout() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let font_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/fonts/NotoSans-Regular.ttf");

    font_system
        .db_mut()
        .load_font_file(font_path)
        .expect("load font file");
    context.clear_glyph_caches();
    let layout = context.layout(
        &mut font_system,
        TextLayoutOptions::new("Noto", 20.0)
            .with_attrs(Attrs::new().family(Family::Name("Noto Sans"))),
    );

    assert!(!layout.is_empty());
}

#[test]
fn scene_glyph_translation_recomputes_subpixel_cache_key() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
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
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(
        &mut font_system,
        TextLayoutOptions::new("A", 20.0).with_attrs(Attrs::new().weight(Weight::BOLD)),
    );
    if layout.glyphs.is_empty() {
        return;
    }

    assert_eq!(layout.glyphs[0].cache_key.font_weight, Weight::BOLD);
}

#[test]
fn layout_options_pass_hinting_flags_to_glyph_keys() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(
        &mut font_system,
        TextLayoutOptions::new("A", 20.0)
            .with_attrs(Attrs::new().cache_key_flags(CacheKeyFlags::DISABLE_HINTING)),
    );
    if layout.glyphs.is_empty() {
        return;
    }

    assert!(
        layout.glyphs[0]
            .cache_key
            .flags
            .contains(CacheKeyFlags::DISABLE_HINTING)
    );
}

#[test]
fn layout_options_pass_alignment_to_cosmic_buffer() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let left = context.layout(
        &mut font_system,
        TextLayoutOptions::new("A", 20.0).with_size(Some(200.0), None),
    );
    let center = context.layout(
        &mut font_system,
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
fn layout_handles_emoji_sequences_without_panicking() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(
        &mut font_system,
        TextLayoutOptions::new("Emoji 😀 👍🏽 👨‍👩‍👧‍👦 🇺🇸", 32.0),
    );
    if layout.glyphs.is_empty() {
        return;
    }

    assert!(!layout.bounds().is_empty());
}

#[test]
fn layout_outline_path_extracts_scalable_glyph_paths() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("Outline", 42.0));
    if layout.is_empty() {
        return;
    }

    let path = context.layout_outline_path(&mut font_system, &layout, Point::new(8.0, 48.0));

    assert!(!path.is_empty());
    assert!(path.bounding_box().width() > 1.0);
    assert!(path.bounding_box().height() > 1.0);
}

#[test]
fn scene_path_text_uses_path_draws_not_glyph_atlas() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("Path", 36.0));
    if layout.is_empty() {
        return;
    }

    let mut canvas = Canvas::new(180, 80, 1.0);
    canvas.push_text_layout_as_path(
        &mut context,
        &mut font_system,
        &layout,
        Point::new(8.0, 52.0),
        Color::BLACK,
        Affine::IDENTITY,
        0.1,
    );

    assert!(!canvas.path_records.is_empty());
    assert!(canvas.text_glyphs.is_empty());
    assert!(canvas.text_runs.is_empty());
    assert!(canvas.draw_records.iter().any(|draw| draw.has_path()));
    assert_eq!(canvas.draw_records[0].tag, DrawTag::PathGlyph);
}

#[test]
fn prepared_text_keeps_color_emoji_glyphs_when_font_supports_them() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("😀", 64.0));
    if layout.is_empty() {
        return;
    }

    let glyphs: Vec<_> = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0)).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: glyphs.len() as u32,
    }];
    let prepared = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);
    let Some(image) = prepared
        .images()
        .iter()
        .find(|image| image.content == PreparedGlyphContent::Color)
    else {
        return;
    };

    assert_eq!(
        image.data.len(),
        image.width as usize * image.height as usize * 4
    );
    assert!(image.data.chunks_exact(4).any(|px| px[3] != 0));
}

#[test]
fn prepared_text_signature_changes_with_glyph_images() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let a = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    let b = context.layout(&mut font_system, TextLayoutOptions::new("B", 20.0));
    if a.is_empty() || b.is_empty() {
        return;
    }

    let a_glyphs: Vec<_> = scene_glyphs_at_origin(&a, Point::new(0.0, 0.0)).collect();
    let b_glyphs: Vec<_> = scene_glyphs_at_origin(&b, Point::new(0.0, 0.0)).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: 1,
    }];
    let a_data = PreparedTextData::new(&a_glyphs, &runs, &mut font_system, &mut context);
    let b_data = PreparedTextData::new(&b_glyphs, &runs, &mut font_system, &mut context);

    assert_ne!(a_data.atlas_signature(), b_data.atlas_signature());
}

#[test]
fn prepared_text_signature_changes_with_composite_mode() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    if layout.is_empty() {
        return;
    }

    let glyphs: Vec<_> = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0)).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: 1,
    }];
    context.set_raster_options(
        TextRasterOptions::new().with_composite_mode(TextCompositeMode::Linear),
    );
    let linear = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);
    context
        .set_raster_options(TextRasterOptions::new().with_composite_mode(TextCompositeMode::Srgb));
    let srgb = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);

    assert_ne!(linear.atlas_signature(), srgb.atlas_signature());
}

#[test]
fn prepared_text_signature_changes_with_subpixel_mode() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    if layout.is_empty() {
        return;
    }

    let glyphs: Vec<_> = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0)).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: 1,
    }];
    context.set_raster_options(TextRasterOptions::new().with_subpixel_mode(TextSubpixelMode::Rgb));
    let rgb = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);
    context.set_raster_options(TextRasterOptions::new().with_subpixel_mode(TextSubpixelMode::Bgr));
    let bgr = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);

    assert_ne!(rgb.atlas_signature(), bgr.atlas_signature());
}

#[test]
fn text_context_uses_subpixel_raster_by_default() {
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("H", 12.0));
    let Some(glyph) = layout.glyphs.first() else {
        return;
    };
    let Some(image) = context.glyph_image(&mut font_system, glyph.cache_key) else {
        return;
    };

    assert_eq!(image.content, SwashContent::SubpixelMask);
}

#[test]
fn harmony_lcd_outline_shifts_follow_freetype_default_geometry() {
    assert_eq!(
        harmony_lcd_outline_shifts(TextSubpixelMode::Rgb),
        [FREETYPE_HARMONY_LCD_SHIFT, 0.0, -FREETYPE_HARMONY_LCD_SHIFT]
    );
    assert_eq!(
        harmony_lcd_outline_shifts(TextSubpixelMode::Bgr),
        [-FREETYPE_HARMONY_LCD_SHIFT, 0.0, FREETYPE_HARMONY_LCD_SHIFT]
    );
}

#[test]
fn harmony_lcd_merge_unions_shifted_channel_masks() {
    let channel = |left, top, width, height, data| {
        let mut image = SwashImage::new();
        image.content = SwashContent::Mask;
        image.placement = Placement {
            left,
            top,
            width,
            height,
        };
        image.data = data;
        image
    };

    let merged = merge_harmony_lcd_masks([
        channel(1, 3, 2, 1, vec![10, 11]),
        channel(0, 2, 1, 2, vec![20, 21]),
        channel(2, 4, 1, 1, vec![30]),
    ]);

    assert_eq!(merged.content, SwashContent::SubpixelMask);
    assert_eq!(merged.placement.left, 0);
    assert_eq!(merged.placement.top, 4);
    assert_eq!(merged.placement.width, 3);
    assert_eq!(merged.placement.height, 4);
    let channel_at =
        |row: usize, col: usize, channel: usize| merged.data[(row * 3 + col) * 3 + channel];
    assert_eq!(channel_at(0, 2, 2), 30);
    assert_eq!(channel_at(1, 1, 0), 10);
    assert_eq!(channel_at(1, 2, 0), 11);
    assert_eq!(channel_at(2, 0, 1), 20);
    assert_eq!(channel_at(3, 0, 1), 21);
}

#[test]
fn prepared_glyph_image_keeps_harmony_subpixel_channels_without_filtering() {
    let mut image = SwashImage::new();
    image.content = SwashContent::SubpixelMask;
    image.placement = swash::zeno::Placement {
        left: 0,
        top: 0,
        width: 1,
        height: 1,
    };
    image.data = vec![255, 0, 0, 255];

    let prepared = PreparedGlyphImage::from_swash(
        &image,
        TextRasterOptions::new()
            .with_subpixel_mode(TextSubpixelMode::Rgb)
            .with_composite_mode(TextCompositeMode::Linear),
    );

    assert_eq!(prepared.content, PreparedGlyphContent::SubpixelMask);
    assert_eq!(prepared.composite_mode, TextCompositeMode::Linear);
    assert_eq!(prepared.left, 0);
    assert_eq!(prepared.width, 1);
    assert_eq!(prepared.data, vec![255, 0, 0]);
}

#[test]
fn prepared_glyph_image_keeps_three_byte_subpixel_masks() {
    let image = GlyphRasterImage::subpixel_mask(
        Placement {
            left: 0,
            top: 0,
            width: 1,
            height: 1,
        },
        vec![255, 0, 0],
    );

    let prepared = PreparedGlyphImage::from_raster(
        &image,
        TextRasterOptions::new().with_subpixel_mode(TextSubpixelMode::Rgb),
    );

    assert_eq!(prepared.content, PreparedGlyphContent::SubpixelMask);
    assert_eq!(prepared.data, vec![255, 0, 0]);
}

#[test]
fn text_raster_options_use_linear_compositing_by_default() {
    assert_eq!(
        TextRasterOptions::default().composite_mode,
        TextCompositeMode::Linear
    );
}
