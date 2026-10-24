use super::*;

#[test]
fn scene_glyph_translation_recomputes_subpixel_cache_key() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("A", 20.0));
    assert!(!layout.glyphs.is_empty());

    let a = scene_glyphs_at_origin(&layout, Point::new(0.0, 0.0))
        .next()
        .unwrap();
    let b = scene_glyphs_at_origin(&layout, Point::new(0.5, 0.0))
        .next()
        .unwrap();
    assert_ne!(a.cache_key, b.cache_key);
}

#[test]
fn scene_path_text_uses_path_draws_not_glyph_atlas() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("Path", 36.0));
    assert!(!layout.is_empty());

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
fn clipped_text_draw_uses_scaled_pixel_bounds_without_a_clip_layer() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(
        &mut font_system,
        TextLayoutOptions::new("A long text run", 24.0).with_wrap(Wrap::None),
    );
    assert!(!layout.is_empty());

    let origin = Point::new(0.0, 24.0);
    let mut unclipped = Canvas::new(200, 80, 2.0);
    unclipped.push_text_layout(&layout, origin, Color::BLACK);
    let natural = unclipped.draw_records[0].pixel_bounds;
    let expected = natural.intersect(PixelBounds {
        x0: 20,
        y0: -2_000,
        x1: 60,
        y1: 2_000,
    });
    assert!(!expected.is_empty());

    let mut canvas = Canvas::new(200, 80, 2.0);
    canvas.push_text_layout_clipped(
        &layout,
        origin,
        Rect::new(10.0, -1_000.0, 30.0, 1_000.0),
        Color::BLACK,
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert_eq!(canvas.draw_records[0].tag, DrawTag::Brush);
    assert_eq!(canvas.draw_records[0].pixel_bounds, expected);
    assert_eq!(
        canvas.draw_records[0].local_pixel_bounds,
        canvas.draw_records[0].pixel_bounds
    );
    assert_eq!(canvas.text_glyphs.len(), layout.glyphs.len());
    assert!(canvas.layer_stack.is_empty());
}

#[test]
fn clipped_text_draw_rejects_empty_or_disjoint_bounds_without_appending_glyphs() {
    let mut font_system = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("Text", 24.0));
    assert!(!layout.is_empty());

    let mut canvas = Canvas::new(100, 40, 1.0);
    assert!(
        canvas
            .push_text_layout_clipped(
                &layout,
                Point::new(0.0, 24.0),
                Rect::new(5.0, 5.0, 5.0, 20.0),
                Color::BLACK,
            )
            .is_none()
    );
    assert!(
        canvas
            .push_text_layout_clipped(
                &layout,
                Point::new(0.0, 24.0),
                Rect::new(200.0, 200.0, 220.0, 220.0),
                Color::BLACK,
            )
            .is_none()
    );
    assert!(canvas.draw_records.is_empty());
    assert!(canvas.text_glyphs.is_empty());
}
