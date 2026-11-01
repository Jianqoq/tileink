use super::*;

#[test]
fn text_context_uses_subpixel_raster_by_default() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("H", 12.0));
    let glyph = layout.glyphs.first().expect("bundled font must shape H");
    let image = context
        .glyph_image(&mut font_system, glyph.cache_key)
        .expect("bundled font must rasterize H");

    assert_eq!(image.content, GlyphContent::SubpixelMask);
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
        GlyphRasterImage::new(
            GlyphContent::Mask,
            Placement {
                left,
                top,
                width,
                height,
            },
            data,
        )
    };

    let merged = merge_harmony_lcd_masks([
        channel(1, 3, 2, 1, vec![10, 11]),
        channel(0, 2, 1, 2, vec![20, 21]),
        channel(2, 4, 1, 1, vec![30]),
    ]);

    assert_eq!(merged.content, GlyphContent::SubpixelMask);
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
fn text_raster_options_use_linear_compositing_by_default() {
    assert_eq!(
        TextRasterOptions::default().composite_mode,
        TextCompositeMode::Linear
    );
}

#[test]
fn raster_contract_rejects_retired_swash_lcd_format() {
    assert!(
        std::panic::catch_unwind(|| {
            GlyphRasterImage::from_swash(raw_glyph(SwashContent::SubpixelMask, vec![1, 2, 3, 255]))
        })
        .is_err()
    );
}

#[test]
fn raster_contract_rejects_invalid_native_lengths() {
    for (content, data) in [
        (SwashContent::Mask, vec![]),
        (SwashContent::Color, vec![1, 2, 3]),
    ] {
        assert!(
            std::panic::catch_unwind(|| GlyphRasterImage::from_swash(raw_glyph(content, data)))
                .is_err()
        );
    }
}

#[test]
fn raster_contract_merge_rejects_color_channels() {
    assert!(
        std::panic::catch_unwind(|| merge_harmony_lcd_masks([
            canonical_glyph(GlyphContent::Color, vec![1, 2, 3, 255]),
            canonical_glyph(GlyphContent::Mask, vec![2]),
            canonical_glyph(GlyphContent::Mask, vec![3]),
        ]))
        .is_err()
    );
}

#[test]
fn raster_contract_merge_rejects_truncated_channels() {
    assert!(
        std::panic::catch_unwind(|| merge_harmony_lcd_masks([
            {
                let mut image = canonical_glyph(GlyphContent::Mask, vec![1]);
                image.data.clear();
                image
            },
            canonical_glyph(GlyphContent::Mask, vec![2]),
            canonical_glyph(GlyphContent::Mask, vec![3]),
        ]))
        .is_err()
    );
}

#[test]
fn canonical_image_layouts_preserve_alpha_color_and_lcd_bytes() {
    for (content, data) in [
        (GlyphContent::Mask, vec![127]),
        (GlyphContent::Color, vec![1, 2, 3, 127]),
        (GlyphContent::SubpixelMask, vec![1, 2, 127]),
    ] {
        let image = canonical_glyph(content, data.clone());
        let prepared = PreparedGlyphImage::from_raster(&image, TextRasterOptions::default());
        assert_eq!(prepared.content, content);
        assert_eq!(prepared.data, data);
        assert_eq!(
            (prepared.left, prepared.top, prepared.width, prepared.height),
            (0, 1, 1, 1)
        );
    }
    for (content, expected, data) in [
        (SwashContent::Mask, GlyphContent::Mask, vec![127]),
        (SwashContent::Color, GlyphContent::Color, vec![1, 2, 3, 127]),
    ] {
        let image = GlyphRasterImage::from_swash(raw_glyph(content, data.clone()));
        assert_eq!(image.content, expected);
        assert_eq!(image.data, data);
    }
}

#[test]
fn canonical_lcd_rejects_four_byte_data() {
    assert!(
        std::panic::catch_unwind(|| canonical_glyph(
            GlyphContent::SubpixelMask,
            vec![1, 2, 3, 255]
        ))
        .is_err()
    );
}

#[test]
fn empty_masks_remain_empty_without_lcd_padding() {
    for (width, height) in [(0, 0), (0, 3), (4, 0)] {
        let placement = Placement {
            left: -2,
            top: 3,
            width,
            height,
        };
        let mask = GlyphRasterImage::new(GlyphContent::Mask, placement, vec![]);
        let lcd = merge_harmony_lcd_masks([mask.clone(), mask.clone(), mask]);
        assert!(lcd.data.is_empty());
        let prepared = PreparedGlyphImage::from_raster(&lcd, TextRasterOptions::default());
        assert!(prepared.data.is_empty());
        assert_eq!((prepared.width, prepared.height), (width, height));
    }
}

#[test]
fn lcd_outline_shifts_require_an_lcd_mode() {
    assert!(
        std::panic::catch_unwind(|| harmony_lcd_outline_shifts(TextSubpixelMode::None)).is_err()
    );
}

#[test]
fn image_byte_contract_cannot_wrap_large_extents() {
    assert!(
        std::panic::catch_unwind(|| GlyphRasterImage::new(
            GlyphContent::Color,
            Placement {
                width: 1 << 31,
                height: 1 << 31,
                ..Placement::default()
            },
            vec![],
        ))
        .is_err()
    );
}
