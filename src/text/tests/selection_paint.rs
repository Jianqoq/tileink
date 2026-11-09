//! Text recoloring is a disjoint pixel partition over one immutable glyph run.
use super::*;

fn contains(bounds: PixelBounds, x: i32, y: i32) -> bool {
    x >= bounds.x0 && x < bounds.x1 && y >= bounds.y0 && y < bounds.y1
}

#[test]
fn highlighted_text_partitions_fractional_overlapping_ranges_without_duplicate_glyphs() {
    let mut fonts = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(
        &mut fonts,
        TextLayoutOptions::new("MMMM office", 24.0).with_wrap(Wrap::None),
    );
    for scale in [1., 1.25, 2., 3.] {
        let origin = Point::new(4.125, 32.25);
        let mut base = Canvas::new(300, 100, scale);
        base.push_text_layout(&layout, origin, Color::BLACK);
        let outer = base.draw_records[0].pixel_bounds;
        let x = f64::from(outer.x0) / f64::from(scale);
        let y = f64::from(outer.y0) / f64::from(scale);
        let regions = [
            Rect::new(x + 4.25, y - 1., x + 15.75, y + 40.),
            Rect::new(x + 10.5, y + 2.5, x + 22.5, y + 9.5),
            Rect::new(x + 10.5, y + 2.5, x + 22.5, y + 9.5),
        ];
        let mut canvas = Canvas::new(300, 100, scale);
        let ids = canvas.push_text_layout_highlighted(
            &layout,
            origin,
            Color::BLACK,
            Color::WHITE,
            &regions,
            None,
        );
        assert_eq!(ids.len(), canvas.draw_records.len());
        assert_eq!(canvas.text_runs.len(), 1);
        assert_eq!(canvas.text_glyphs, base.text_glyphs);
        let normal = canvas.draw_records[0].brush_offset;
        let highlighted = canvas
            .draw_records
            .iter()
            .find(|d| d.brush_offset != normal)
            .unwrap()
            .brush_offset;
        for py in outer.y0..outer.y1 {
            for px in outer.x0..outer.x1 {
                let matching = canvas
                    .draw_records
                    .iter()
                    .filter(|d| contains(d.pixel_bounds, px, py))
                    .collect::<Vec<_>>();
                assert_eq!(
                    matching.len(),
                    1,
                    "{scale}: {px},{py} must composite exactly once"
                );
                let point = Point::new(
                    (f64::from(px) + 0.5) / f64::from(scale),
                    (f64::from(py) + 0.5) / f64::from(scale),
                );
                let selected = regions.iter().any(|r| {
                    point.x >= r.x0 && point.x < r.x1 && point.y >= r.y0 && point.y < r.y1
                });
                assert_eq!(
                    matching[0].brush_offset,
                    if selected { highlighted } else { normal },
                    "{scale}: {point:?}"
                );
                assert_eq!(matching[0].glyph_run_id, 0);
            }
        }
    }
}

#[test]
fn fully_highlighted_text_records_only_the_requested_foreground() {
    let mut fonts = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut fonts, TextLayoutOptions::new("MMMM", 24.));
    for foreground in [Color::WHITE, Color::from_rgba8(5, 20, 40, 0)] {
        let mut expected = Canvas::new(200, 100, 1.);
        expected.push_text_layout(&layout, Point::new(4., 32.), foreground);
        let mut actual = Canvas::new(200, 100, 1.);
        let ids = actual.push_text_layout_highlighted(
            &layout,
            Point::new(4., 32.),
            Color::BLACK,
            foreground,
            &[Rect::new(-1000., -1000., 1000., 1000.)],
            None,
        );
        assert_eq!(ids.len(), 1);
        assert_eq!(actual.brush_blob, expected.brush_blob);
        assert_eq!(actual.text_glyphs, expected.text_glyphs);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&actual.draw_records),
            bytemuck::cast_slice::<_, u8>(&expected.draw_records)
        );
    }
}

#[test]
fn absent_and_invalid_highlights_keep_the_normal_text_recording() {
    let mut fonts = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut fonts, TextLayoutOptions::new("MMMM", 24.));
    let mut expected = Canvas::new(200, 100, 1.);
    expected.push_text_layout(&layout, Point::new(4., 32.), Color::BLACK);
    for regions in [
        vec![],
        vec![Rect::ZERO, Rect::new(f64::NAN, 0., 5., 5.)],
        vec![Rect::new(1000., 1000., 2000., 2000.)],
    ] {
        let mut actual = Canvas::new(200, 100, 1.);
        let ids = actual.push_text_layout_highlighted(
            &layout,
            Point::new(4., 32.),
            Color::BLACK,
            Color::WHITE,
            &regions,
            None,
        );
        assert_eq!(ids.len(), 1);
        assert_eq!(actual.brush_blob, expected.brush_blob);
        assert_eq!(actual.text_glyphs, expected.text_glyphs);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&actual.draw_records),
            bytemuck::cast_slice::<_, u8>(&expected.draw_records)
        );
    }
}

#[test]
fn highlighted_text_respects_outer_clip_and_rejects_empty_output_atomically() {
    let mut fonts = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut fonts, TextLayoutOptions::new("MMMM office", 24.));
    let clip = Rect::new(7.25, 10., 45.75, 35.);
    let mut expected = Canvas::new(200, 100, 2.);
    expected.push_text_layout_clipped(&layout, Point::new(4., 32.), clip, Color::BLACK);
    let outer = expected.draw_records[0].pixel_bounds;
    let mut actual = Canvas::new(200, 100, 2.);
    actual.push_text_layout_highlighted(
        &layout,
        Point::new(4., 32.),
        Color::BLACK,
        Color::WHITE,
        &[Rect::new(12.25, 0., 27.75, 100.)],
        Some(clip),
    );
    for draw in &actual.draw_records {
        assert!(draw.pixel_bounds.x0 >= outer.x0 && draw.pixel_bounds.y0 >= outer.y0);
        assert!(draw.pixel_bounds.x1 <= outer.x1 && draw.pixel_bounds.y1 <= outer.y1);
    }
    assert_eq!(actual.text_runs.len(), 1);
    for clip in [
        Rect::ZERO,
        Rect::new(f64::NAN, 0., 10., 10.),
        Rect::new(1000., 0., 2000., 100.),
    ] {
        let mut empty = Canvas::new(200, 100, 2.);
        assert!(
            empty
                .push_text_layout_highlighted(
                    &layout,
                    Point::new(4., 32.),
                    Color::BLACK,
                    Color::WHITE,
                    &[Rect::new(0., 0., 100., 100.)],
                    Some(clip)
                )
                .is_empty()
        );
        assert!(empty.draw_records.is_empty() && empty.text_glyphs.is_empty());
    }
}
