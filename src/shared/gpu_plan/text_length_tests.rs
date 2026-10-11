use peniko::{Color, kurbo::Point};

use super::GpuBufferLengths;
use crate::{Canvas, TextContext, TextFontSystem, TextLayoutOptions, text::PreparedTextData};

fn fonts() -> TextFontSystem {
    let mut fonts = TextFontSystem::new_with_locale_and_db("en-US".into(), Default::default());
    fonts
        .db_mut()
        .load_font_data(include_bytes!("../../svg/fonts/NotoSans-Regular.ttf").to_vec());
    fonts.db_mut().set_sans_serif_family("Noto Sans");
    fonts
}

#[test]
fn glyph_capacity_covers_bilinear_neighbours_before_transforming() {
    use crate::shared::{affine::GpuAffine, bounds::Bounds};
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(
        peniko::kurbo::Rect::new(0.0, 0.0, 64.0, 64.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let mut draw = canvas.draw_records[0];
    let bounds = Bounds::new(16, 16, 32, 32);
    for (transform, expected) in [
        (GpuAffine::IDENTITY, Bounds::new(15, 15, 33, 33)),
        (
            GpuAffine {
                e: 0.25,
                f: -0.25,
                ..GpuAffine::IDENTITY
            },
            Bounds::new(15, 14, 34, 33),
        ),
        (
            GpuAffine {
                a: 2.0,
                d: 0.5,
                ..GpuAffine::IDENTITY
            },
            Bounds::new(30, 7, 66, 17),
        ),
    ] {
        draw.transform = transform;
        assert_eq!(super::transformed_glyph_bounds(&draw, bounds), expected);
    }
}

#[test]
fn text_lengths_count_only_tiles_intersecting_glyph_bounds() {
    let mut font_system = fonts();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("MMMMMMMM", 24.0));
    assert!(!layout.is_empty());

    let mut canvas = Canvas::new(160, 64, 1.0);
    canvas.push_text_layout(&layout, Point::new(2.0, 32.0), Color::WHITE);
    let text = PreparedTextData::new(
        &canvas.text_glyphs,
        &canvas.text_runs,
        &mut font_system,
        &mut context,
    );
    let lengths = GpuBufferLengths::from_scene_with_text(&canvas, Some(&text));

    let expected = text
        .run_glyph_indices(0)
        .filter_map(|glyph_id| {
            let bounds = text.glyph_bounds(glyph_id)?.outset(1);
            Some(super::bounds_tile_bbox(
                bounds,
                canvas.width_in_tiles(),
                canvas.height_in_tiles(),
            ))
        })
        .map(|bbox| bbox.tile_count() as usize)
        .sum::<usize>();

    assert_eq!(lengths.coarse_glyph_capacity, expected);
}

#[test]
fn text_lengths_track_whether_gpu_text_data_was_prepared() {
    let mut font_system = fonts();
    let mut context = TextContext::new();
    let layout = context.layout(&mut font_system, TextLayoutOptions::new("text", 24.0));
    assert!(!layout.is_empty());

    let mut canvas = Canvas::new(96, 48, 1.0);
    canvas.push_text_layout(&layout, Point::new(2.0, 28.0), Color::WHITE);
    let no_text_lengths = GpuBufferLengths::from_scene(&canvas);
    let text = PreparedTextData::new(
        &canvas.text_glyphs,
        &canvas.text_runs,
        &mut font_system,
        &mut context,
    );
    let text_lengths = GpuBufferLengths::from_scene_with_text(&canvas, Some(&text));

    assert!(!no_text_lengths.text_enabled);
    assert!(text_lengths.text_enabled);
}
