use super::*;

#[test]
fn prepared_text_keeps_bundled_color_emoji_glyphs() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("😀", 64.0));
    assert!(!layout.is_empty());

    let glyphs: Vec<_> = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0)).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: glyphs.len() as u32,
    }];
    for mode in [
        TextSubpixelMode::None,
        TextSubpixelMode::Rgb,
        TextSubpixelMode::Bgr,
    ] {
        context.set_raster_options(TextRasterOptions::new().with_subpixel_mode(mode));
        let prepared = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);
        let image = prepared
            .images()
            .iter()
            .find(|image| image.content == GlyphContent::Color)
            .expect("bundled emoji font must rasterize a color image");
        assert_eq!(
            image.data.len(),
            image.width as usize * image.height as usize * 4
        );
        assert!(image.data.as_chunks::<4>().0.iter().any(|px| px[3] != 0));
    }
}

#[test]
fn prepared_text_signature_changes_with_glyph_images() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let a = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    let b = context.layout(&mut font_system, TextLayoutOptions::new("B", 20.0));
    assert!(!a.is_empty() && !b.is_empty());

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
fn prepared_text_updates_only_dirty_glyph_slots_without_rebuilding_atlas() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("AB", 20.0));
    assert!(!layout.is_empty());
    let mut glyphs: Vec<_> = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0)).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: glyphs.len() as u32,
    }];
    let mut prepared = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);
    let signature = prepared.atlas_signature();
    let first = prepared.glyph(0).map(|glyph| (glyph.x, glyph.y));
    let changed = glyphs.len() - 1;
    glyphs[changed].x += 13;

    prepared.update(
        &glyphs,
        &runs,
        std::slice::from_ref(&(changed..changed + 1)),
        &[],
        &mut font_system,
        &mut context,
    );

    assert_eq!(prepared.atlas_signature(), signature);
    assert_eq!(prepared.glyph(0).map(|glyph| (glyph.x, glyph.y)), first);
    assert_eq!(prepared.glyph(changed as u32).unwrap().x, glyphs[changed].x);
}

#[test]
fn prepared_text_reconcile_reuses_images_for_position_only_flat_canvas() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(
        &mut font_system,
        TextLayoutOptions::new("Market Watch", 20.0),
    );
    assert!(!layout.is_empty());
    let glyphs: Vec<_> = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0)).collect();
    let mut moved = glyphs.clone();
    for glyph in &mut moved {
        glyph.x += 12;
        glyph.y += 7;
    }
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: glyphs.len() as u32,
    }];
    let mut prepared = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);
    let signature = prepared.atlas_signature();
    let image_ids = (0..glyphs.len() as u32)
        .map(|index| prepared.glyph(index).unwrap().image)
        .collect::<Vec<_>>();

    let changes = prepared
        .reconcile(&moved, &runs, &mut font_system, &mut context)
        .expect("position-only reconciliation should stay incremental");

    assert_eq!(prepared.atlas_signature(), signature);
    assert_eq!(changes.glyphs().len(), 1);
    assert_eq!(changes.glyphs()[0], 0..moved.len());
    assert!(changes.runs().is_empty());
    for (index, glyph) in moved.iter().enumerate() {
        let actual = prepared.glyph(index as u32).unwrap();
        assert_eq!(actual.image, image_ids[index]);
        assert_eq!((actual.x, actual.y), (glyph.x, glyph.y));
    }
}

#[test]
fn prepared_text_reconcile_adds_only_new_glyph_images() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let a = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    let ab = context.layout(&mut font_system, TextLayoutOptions::new("AB", 20.0));
    assert!(!a.is_empty() && ab.glyphs.len() >= 2);
    let a_glyphs: Vec<_> = scene_glyphs_at_origin(&a, Point::new(0.0, 0.0)).collect();
    let ab_glyphs: Vec<_> = scene_glyphs_at_origin(&ab, Point::new(0.0, 0.0)).collect();
    let a_runs = [TextRun {
        glyph_start: 0,
        glyph_count: a_glyphs.len() as u32,
    }];
    let ab_runs = [TextRun {
        glyph_start: 0,
        glyph_count: ab_glyphs.len() as u32,
    }];
    let mut prepared = PreparedTextData::new(&a_glyphs, &a_runs, &mut font_system, &mut context);
    let a_image = prepared.glyph(0).unwrap().image;
    let old_image_count = prepared.images().len();

    let _ = prepared.reconcile(&ab_glyphs, &ab_runs, &mut font_system, &mut context);

    assert_eq!(prepared.glyph(0).unwrap().image, a_image);
    assert!(prepared.images().len() >= old_image_count);
    assert_eq!(prepared.run_glyph_indices(0), 0..ab_glyphs.len() as u32);
}

#[test]
fn prepared_text_reconcile_reports_same_length_glyph_replacement() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let a = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    let b = context.layout(&mut font_system, TextLayoutOptions::new("B", 20.0));
    assert!(a.glyphs.len() == 1 && b.glyphs.len() == 1);
    let a_glyphs: Vec<_> = scene_glyphs_at_origin(&a, Point::new(0.0, 0.0)).collect();
    let b_glyphs: Vec<_> = scene_glyphs_at_origin(&b, Point::new(0.0, 0.0)).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: 1,
    }];
    let mut prepared = PreparedTextData::new(&a_glyphs, &runs, &mut font_system, &mut context);

    let changes = prepared
        .reconcile(&b_glyphs, &runs, &mut font_system, &mut context)
        .expect("same-length glyph replacement should stay incremental");

    assert_eq!(changes.glyphs().len(), 1);
    assert_eq!(changes.glyphs()[0], 0..1);
    assert!(changes.runs().is_empty());
    assert_eq!(prepared.glyph(0).unwrap().cache_key, b_glyphs[0].cache_key);
}

#[test]
fn clearing_text_context_invalidates_prepared_atlas_generation() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    assert!(!layout.is_empty());
    let glyphs: Vec<_> = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0)).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: glyphs.len() as u32,
    }];
    let mut prepared = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);
    let generation = prepared.cache_generation();
    let signature = prepared.atlas_signature();

    context.clear_glyph_caches();
    let changes = prepared.reconcile(&glyphs, &runs, &mut font_system, &mut context);

    assert!(changes.is_none());
    assert_ne!(prepared.cache_generation(), generation);
    assert_ne!(prepared.atlas_signature(), signature);
}

#[test]
fn prepared_text_reconcile_truncates_removed_glyphs_and_runs() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("AB", 20.0));
    assert!(layout.glyphs.len() >= 2);
    let glyphs: Vec<_> = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0)).collect();
    let runs = [
        TextRun {
            glyph_start: 0,
            glyph_count: 1,
        },
        TextRun {
            glyph_start: 1,
            glyph_count: (glyphs.len() - 1) as u32,
        },
    ];
    let mut prepared = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);

    let _ = prepared.reconcile(&glyphs[..1], &runs[..1], &mut font_system, &mut context);

    assert!(prepared.glyph(1).is_none());
    assert_eq!(prepared.run_glyph_indices(1), 0..0);
}

#[test]
fn prepared_text_reconcile_rebuilds_when_raster_options_change() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    assert!(!layout.is_empty());
    let glyphs: Vec<_> = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0)).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: glyphs.len() as u32,
    }];
    let mut prepared = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);
    let signature = prepared.atlas_signature();
    context
        .set_raster_options(TextRasterOptions::new().with_composite_mode(TextCompositeMode::Srgb));

    let changes = prepared.reconcile(&glyphs, &runs, &mut font_system, &mut context);

    assert!(changes.is_none());
    assert_ne!(prepared.atlas_signature(), signature);
}

#[test]
fn prepared_text_budget_rebuild_discards_replaced_glyph_images() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let a = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    let b = context.layout(&mut font_system, TextLayoutOptions::new("B", 20.0));
    assert!(a.glyphs.len() == 1 && b.glyphs.len() == 1);
    let a_glyphs: Vec<_> = scene_glyphs_at_origin(&a, Point::new(0.0, 0.0)).collect();
    let b_glyphs: Vec<_> = scene_glyphs_at_origin(&b, Point::new(0.0, 0.0)).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: 1,
    }];
    let b_probe = PreparedTextData::new(&b_glyphs, &runs, &mut font_system, &mut context);
    let mut prepared = PreparedTextData::new(&a_glyphs, &runs, &mut font_system, &mut context);
    assert!(
        prepared
            .image_id_for_cache_key(a_glyphs[0].cache_key)
            .is_some()
            && b_probe
                .image_id_for_cache_key(b_glyphs[0].cache_key)
                .is_some()
    );

    let changes =
        prepared.reconcile_with_image_budget(&b_glyphs, &runs, &mut font_system, &mut context, 0);

    assert!(changes.is_none());
    assert!(
        prepared
            .image_id_for_cache_key(a_glyphs[0].cache_key)
            .is_none()
    );
    assert!(
        prepared
            .image_id_for_cache_key(b_glyphs[0].cache_key)
            .is_some()
    );
    assert_eq!(prepared.images().len(), 1);
}

#[test]
fn prepared_text_signature_changes_with_composite_mode() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    assert!(!layout.is_empty());

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
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    assert!(!layout.is_empty());

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
fn prepared_glyph_image_preserves_filtered_harmony_subpixel_channels() {
    let mut image = canonical_glyph(GlyphContent::SubpixelMask, vec![255, 0, 0]);
    filter_harmony_lcd_mask(&mut image, TextSubpixelMode::Rgb);
    let prepared = PreparedGlyphImage::from_raster(
        &image,
        TextRasterOptions::new().with_composite_mode(TextCompositeMode::Linear),
    );
    assert_eq!(prepared.content, GlyphContent::SubpixelMask);
    assert_eq!(prepared.composite_mode, TextCompositeMode::Linear);
    assert_eq!(prepared.left, -1);
    assert_eq!(prepared.width, 3);
    assert_eq!(prepared.data, image.data);
    assert_eq!(prepared.data, vec![0, 0, 21, 213, 21, 0, 0, 0, 0]);
}

#[test]
fn prepared_glyph_image_keeps_three_byte_subpixel_masks() {
    let image = GlyphRasterImage::new(
        GlyphContent::SubpixelMask,
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

    assert_eq!(prepared.content, GlyphContent::SubpixelMask);
    assert_eq!(prepared.data, vec![255, 0, 0]);
}
