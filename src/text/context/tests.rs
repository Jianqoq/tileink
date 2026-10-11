use super::*;
use crate::text::{GlyphContent, TextSubpixelMode, tests::font_system};

#[test]
fn image_cache_reuses_data_and_separates_raster_options() {
    let mut fonts = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut fonts, TextLayoutOptions::new("H", 24.0));
    let key = layout.glyphs()[0].cache_key;
    context.clear_glyph_caches();
    for (index, mode) in [
        TextSubpixelMode::Rgb,
        TextSubpixelMode::Bgr,
        TextSubpixelMode::None,
    ]
    .into_iter()
    .enumerate()
    {
        context.set_raster_options(TextRasterOptions::new().with_subpixel_mode(mode));
        let image = context.glyph_image(&mut fonts, key).unwrap();
        let pointer = image.data.as_ptr();
        assert!(!image.data.is_empty());
        assert_eq!(
            image.content,
            if mode == TextSubpixelMode::None {
                GlyphContent::Mask
            } else {
                GlyphContent::SubpixelMask
            }
        );
        assert_eq!(
            context.glyph_image(&mut fonts, key).unwrap().data.as_ptr(),
            pointer
        );
        assert_eq!(context.image_cache.len(), index + 1);
    }
    let mut options = context.raster_options();
    options.alpha_mask_embolden += 0.25;
    context.set_raster_options(options);
    assert!(context.glyph_image(&mut fonts, key).is_some());
    assert_eq!(context.image_cache.len(), 4);
}

#[test]
fn mask_weight_options_change_pixels_and_reuse_only_the_selected_mode() {
    let mut fonts = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut fonts, TextLayoutOptions::new("H", 24.0));
    let key = layout.glyphs()[0].cache_key;
    for mode in [
        TextSubpixelMode::None,
        TextSubpixelMode::Rgb,
        TextSubpixelMode::Bgr,
    ] {
        context.clear_glyph_caches();
        let options = TextRasterOptions::new().with_subpixel_mode(mode);
        context.set_raster_options(options);
        let original = context.glyph_image(&mut fonts, key).unwrap().data.clone();
        let unused = if mode == TextSubpixelMode::None {
            options.with_subpixel_mask_embolden(0.75)
        } else {
            options.with_alpha_mask_embolden(0.75)
        };
        context.set_raster_options(unused);
        assert_eq!(context.glyph_image(&mut fonts, key).unwrap().data, original);
        assert_eq!(context.image_cache.len(), 1);
        let changed = options
            .with_alpha_mask_embolden(0.75)
            .with_subpixel_mask_embolden(0.75);
        context.set_raster_options(changed);
        assert_ne!(context.glyph_image(&mut fonts, key).unwrap().data, original);
        assert_eq!(context.image_cache.len(), 2);
        context.set_raster_options(options);
        assert_eq!(context.glyph_image(&mut fonts, key).unwrap().data, original);
        assert_eq!(context.image_cache.len(), 2);
    }
}

#[test]
fn outline_cache_ignores_subpixel_bins_and_preserves_translation() {
    let mut fonts = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut fonts, TextLayoutOptions::new("H", 24.0));
    let key = layout.glyphs()[0].cache_key;
    let pointer = context
        .glyph_outline_path(&mut fonts, key)
        .unwrap()
        .elements()
        .as_ptr();
    let shifted = CacheKey {
        x_bin: cosmic_text::SubpixelBin::Two,
        y_bin: cosmic_text::SubpixelBin::Three,
        ..key
    };
    assert_eq!(
        context
            .glyph_outline_path(&mut fonts, shifted)
            .unwrap()
            .elements()
            .as_ptr(),
        pointer
    );
    assert_eq!(context.outline_cache.len(), 1);
    let a = context.layout_outline_path(&mut fonts, &layout, Point::ORIGIN);
    let b = context.layout_outline_path(&mut fonts, &layout, Point::new(7.5, -9.25));
    assert!(!a.is_empty());
    assert_eq!(b, peniko::kurbo::Affine::translate((7.5, -9.25)) * a);
}

#[test]
fn missing_glyph_results_are_cached_and_clear_invalidates_both_caches() {
    let mut fonts = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut fonts, TextLayoutOptions::new("H", 24.0));
    let valid = layout.glyphs()[0].cache_key;
    let missing = CacheKey {
        glyph_id: u16::MAX,
        ..valid
    };
    assert!(context.glyph_outline_path(&mut fonts, valid).is_some());
    for _ in 0..2 {
        assert!(context.glyph_image(&mut fonts, missing).is_none());
        assert!(context.glyph_outline_path(&mut fonts, missing).is_none());
        assert_eq!(context.image_cache.len(), 2);
        assert_eq!(context.outline_cache.len(), 2);
    }
    assert!(context.image_cache.values().any(Option::is_none));
    assert!(context.outline_cache.values().any(Option::is_none));
    let generation = context.cache_generation();
    context.clear_glyph_caches();
    assert!(context.image_cache.is_empty());
    assert!(context.outline_cache.is_empty());
    assert_eq!(context.cache_generation(), generation + 1);
    assert!(context.glyph_image(&mut fonts, valid).is_some());
    assert!(context.glyph_outline_path(&mut fonts, valid).is_some());
}
