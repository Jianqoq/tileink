use super::*;
use crate::Bounds;
use cosmic_text::SubpixelBin;

fn layout() -> (FontSystem, TextContext, TextLayout) {
    let mut fonts = font_system();
    let mut text = TextContext::new();
    let layout = text.layout(&mut fonts, TextLayoutOptions::new("A", 20.0));
    assert!(!layout.is_empty());
    (fonts, text, layout)
}

fn image_bounds(x: i32, y: i32, left: i32, top: i32, width: u32, height: u32) -> Bounds {
    let x = i64::from(x) + i64::from(left);
    let y = i64::from(y) - i64::from(top);
    let clamp = |value: i64| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    Bounds::new(
        clamp(x),
        clamp(y),
        clamp(x + i64::from(width)),
        clamp(y + i64::from(height)),
    )
}

#[test]
fn translated_text_layout_bounds_do_not_wrap_at_coordinate_edges() {
    let (_, _, layout) = layout();
    for origin in [
        -3.0e9,
        f64::from(i32::MIN),
        -0.125,
        0.0,
        0.125,
        f64::from(i32::MAX),
        3.0e9,
    ] {
        let bounds = layout.bounds();
        let actual = layout_bounds_at_scaled_origin(&layout, Point::new(origin, origin), 1.0);
        let expected = Bounds::new(
            (f64::from(bounds.x0) + origin.floor() - 1.0) as i32,
            (f64::from(bounds.y0) + origin.floor() - 1.0) as i32,
            (f64::from(bounds.x1) + origin.ceil() + 1.0) as i32,
            (f64::from(bounds.y1) + origin.ceil() + 1.0) as i32,
        );
        assert_eq!(actual, expected, "origin={origin}");
    }
}

#[test]
fn scaled_text_layout_bounds_transform_before_coordinate_clipping() {
    let (_, _, layout) = layout();
    for (origin, scale) in [
        (4294967296.0f64, 2.0f32.powi(-28)),
        (-4294967296.0, 2.0f32.powi(-28)),
        (12.125, 2.75),
        (0.0, 1.0e30),
    ] {
        let bounds = layout.bounds();
        let pad = f64::from(scale).ceil() + 2.0;
        let lower = |value| {
            (((f64::from(value) + origin.floor() - 1.0) * f64::from(scale)).floor() - pad) as i32
        };
        let upper = |value| {
            (((f64::from(value) + origin.ceil() + 1.0) * f64::from(scale)).ceil() + pad) as i32
        };
        assert_eq!(
            layout_bounds_at_scaled_origin(&layout, Point::new(origin, origin), scale),
            Bounds::new(
                lower(bounds.x0),
                lower(bounds.y0),
                upper(bounds.x1),
                upper(bounds.y1)
            ),
            "origin={origin}, scale={scale}"
        );
    }
}

#[test]
fn text_glyph_cache_positions_saturate_without_wrapping() {
    let (_, _, layout) = layout();
    let glyph = scene_glyphs_at_origin(&layout, Point::ZERO).next().unwrap();
    for origin in [-f64::MAX, -3.0e9, 3.0e9, f64::MAX] {
        let expected = if origin < 0.0 { i32::MIN } else { i32::MAX };
        for scale in [1.0, 2.0] {
            let positioned =
                scene_glyphs_at_scaled_origin(&layout, Point::new(origin, origin), scale)
                    .next()
                    .unwrap();
            assert_eq!((positioned.x, positioned.y), (expected, expected));
            assert_eq!(
                (positioned.cache_key.x_bin, positioned.cache_key.y_bin),
                (SubpixelBin::Zero, SubpixelBin::Zero)
            );
        }
        let translated = glyph.translated(origin, origin);
        assert_eq!((translated.x, translated.y), (expected, expected));
        assert_eq!(
            (translated.cache_key.x_bin, translated.cache_key.y_bin),
            (SubpixelBin::Zero, SubpixelBin::Zero)
        );
    }
}

#[test]
fn raster_glyph_bounds_clamp_after_complete_endpoint_arithmetic() {
    let (mut fonts, mut text, layout) = layout();
    let mut glyph = layout.glyphs()[0];
    let image = text.glyph_image(&mut fonts, glyph.cache_key).unwrap();
    let p = image.placement;
    for (x, y) in [
        (i32::MIN, i32::MIN),
        (i32::MAX, i32::MAX),
        (i32::MAX - 3, i32::MIN + 3),
        (-7, 13),
    ] {
        glyph.x = x;
        glyph.y = y;
        assert_eq!(
            glyph.image_bounds(image),
            image_bounds(x, y, p.left, p.top, p.width, p.height)
        );
    }
}

#[test]
fn prepared_glyph_bounds_clamp_after_complete_endpoint_arithmetic() {
    let (mut fonts, mut text, layout) = layout();
    let mut glyphs: Vec<_> = scene_glyphs_at_origin(&layout, Point::ZERO).collect();
    let runs = [TextRun {
        glyph_start: 0,
        glyph_count: glyphs.len() as u32,
    }];
    for (x, y) in [
        (i32::MIN, i32::MIN),
        (i32::MAX, i32::MAX),
        (i32::MAX - 3, i32::MIN + 3),
        (-7, 13),
    ] {
        glyphs[0].x = x;
        glyphs[0].y = y;
        let prepared = PreparedTextData::new(&glyphs, &runs, &mut fonts, &mut text);
        let image = prepared
            .image(prepared.glyph(0).unwrap().image.unwrap())
            .unwrap();
        assert_eq!(
            prepared.glyph_bounds(0).unwrap(),
            image_bounds(x, y, image.left, image.top, image.width, image.height)
        );
    }
}

#[test]
fn nonfinite_text_origins_do_not_record_substitute_positions() {
    let (_, _, layout) = layout();
    for origin in [
        Point::new(f64::NAN, 20.0),
        Point::new(0.0, f64::NAN),
        Point::new(f64::INFINITY, 20.0),
        Point::new(0.0, f64::NEG_INFINITY),
    ] {
        let mut canvas = Canvas::new(80, 40, 1.0);
        assert!(
            canvas
                .push_text_layout(&layout, origin, Color::WHITE)
                .is_none()
        );
        assert!(
            canvas
                .push_text_layout_clipped(
                    &layout,
                    origin,
                    Rect::new(0.0, 0.0, 80.0, 40.0),
                    Color::WHITE
                )
                .is_none()
        );
        assert!(canvas.draw_records.is_empty());
        assert!(canvas.text_glyphs.is_empty());
        assert!(canvas.text_runs.is_empty());
        assert!(canvas.brush_blob.is_empty());
    }
}

#[test]
fn text_outside_the_coordinate_domain_does_not_append_empty_runs() {
    let (_, _, layout) = layout();
    for origin in [-f64::MAX, -3.0e9, 3.0e9, f64::MAX] {
        let mut canvas = Canvas::new(80, 40, 1.0);
        assert!(
            canvas
                .push_text_layout(&layout, Point::new(origin, origin), Color::WHITE)
                .is_none()
        );
        assert!(canvas.draw_records.is_empty());
        assert!(canvas.text_glyphs.is_empty());
        assert!(canvas.text_runs.is_empty());
        assert!(canvas.brush_blob.is_empty());
    }
}

#[test]
fn text_translation_preserves_fractional_offsets_after_large_coordinate_cancellation() {
    let (_, _, layout) = layout();
    let glyph = scene_glyphs_at_origin(&layout, Point::ZERO).next().unwrap();
    for offset in [-0.5, -0.25, 0.25, 0.5] {
        let large = 16777216.0;
        let actual = glyph
            .translated(large + offset, 0.0)
            .translated(-large, 0.0);
        assert_eq!(actual, glyph.translated(offset, 0.0), "offset={offset}");
    }
}

#[test]
fn text_position_binning_preserves_cosmic_text_thresholds_and_key_metadata() {
    let (_, _, layout) = layout();
    let glyph = scene_glyphs_at_origin(&layout, Point::ZERO).next().unwrap();
    for whole in [-4096.0f32, -128.0, -1.0, 0.0, 1.0, 128.0, 4096.0] {
        for fraction in [
            -0.8751f32, -0.875, -0.625, -0.375, -0.1251, -0.125, 0.0, 0.1249, 0.125, 0.375, 0.625,
            0.875, 0.8751,
        ] {
            let position = whole + fraction;
            let (cache_key, x, y) = cosmic_text::CacheKey::new(
                glyph.cache_key.font_id,
                glyph.cache_key.glyph_id,
                f32::from_bits(glyph.cache_key.font_size_bits),
                (position, position),
                glyph.cache_key.font_weight,
                glyph.cache_key.flags,
            );
            let actual = glyph.translated(
                f64::from(position)
                    - f64::from(glyph.x)
                    - f64::from(glyph.cache_key.x_bin.as_float()),
                f64::from(position)
                    - f64::from(glyph.y)
                    - f64::from(glyph.cache_key.y_bin.as_float()),
            );
            assert_eq!(
                actual,
                CanvasGlyph { cache_key, x, y },
                "position={position}"
            );
        }
    }
}

#[test]
fn layout_buffer_preserves_cosmic_physical_keys_and_vertical_hinting() {
    let mut fonts = font_system();
    let mut text = TextContext::new();
    for font_size in [12.0, 20.25, 32.5] {
        let mut buffer = cosmic_text::Buffer::new(
            &mut fonts,
            cosmic_text::Metrics::new(font_size, font_size * 1.2),
        );
        buffer.set_size(Some(200.25), None);
        buffer.set_wrap(Wrap::None);
        buffer.set_text(
            "A\nHi😀",
            &Attrs::new().weight(Weight::BOLD),
            cosmic_text::Shaping::Advanced,
            Some(Align::Center),
        );
        buffer.shape_until_scroll(&mut fonts, false);
        let expected: Vec<_> = buffer
            .layout_runs()
            .flat_map(|run| {
                run.glyphs
                    .iter()
                    .map(move |glyph| glyph.physical((0.0, run.line_y), 1.0))
            })
            .collect();
        let layout = text.layout_buffer(&mut fonts, &mut buffer);
        assert!(!expected.is_empty());
        assert_eq!(layout.glyphs().len(), expected.len());
        for (actual, expected) in layout.glyphs().iter().zip(expected) {
            assert_eq!(actual.cache_key, expected.cache_key);
            assert_eq!((actual.x, actual.y), (expected.x, expected.y));
        }
    }
}

#[test]
fn aligned_layout_does_not_wrap_out_of_domain_cosmic_positions() {
    let mut fonts = font_system();
    let mut text = TextContext::new();
    for width in [1.0e10, f32::MAX] {
        let layout = text.layout(
            &mut fonts,
            TextLayoutOptions::new("A", 20.0)
                .with_size(Some(width), None)
                .with_wrap(Wrap::None)
                .with_alignment(Some(Align::Center)),
        );
        assert!(!layout.glyphs().is_empty());
        for glyph in layout.glyphs() {
            assert_eq!(glyph.x, i32::MAX, "width={width}");
            assert_eq!(glyph.cache_key.x_bin, SubpixelBin::Zero);
            assert!(glyph.outline_origin.x > f64::from(i32::MAX));
        }
    }
}
