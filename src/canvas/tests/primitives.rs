use super::*;
use crate::{
    TextLayoutOptions,
    shared::{
        image::{Image, premul_color_to_rgba8_pack},
        pixel::premul_f32_to_u32,
        scene_columns::GPU_BRUSH_U32_STRIDE,
    },
};

#[test]
fn push_rect_records_sdf_rect_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    let draw = &canvas.draw_records[0];
    assert_eq!(
        draw.pixel_bounds,
        PixelBounds {
            x0: 2,
            y0: 3,
            x1: 18,
            y1: 19,
        }
    );
    assert_eq!(draw.tag, DrawTag::Brush);
    assert!(draw.path_id.is_none());
    assert!(!draw.solid_rect);
    match draw.sdf {
        Some(Sdf::Rect(rect)) => {
            assert_eq!(rect.axis_bounds(), (2.0, 3.0, 18.0, 19.0));
            assert!(rect.radius.is_zero());
        }
        sdf => panic!("expected rect SDF, got {sdf:?}"),
    }
}

#[test]
fn push_rect_records_sdf_rect_with_independent_radii() {
    let mut canvas = test_scene();
    let radius = Radius {
        top_left: 3.0,
        top_right: 9.0,
        bottom_left: 15.0,
        bottom_right: 21.0,
    };
    canvas.push_rect(
        Rect::new(4.0, 5.0, 40.0, 41.0),
        radius,
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    match canvas.draw_records[0].sdf {
        Some(Sdf::Rect(rect)) => {
            assert_eq!(rect.axis_bounds(), (4.0, 5.0, 40.0, 41.0));
            assert_eq!(rect.radius.top_left, 3.0);
            assert_eq!(rect.radius.top_right, 9.0);
            assert_eq!(rect.radius.bottom_left, 15.0);
            assert_eq!(rect.radius.bottom_right, 21.0);
        }
        sdf => panic!("expected rect SDF, got {sdf:?}"),
    }
}

#[test]
fn push_image_records_pattern_rect_draw() {
    let mut canvas = test_scene();
    let image = Image::from_rgba8(2, 1, [255, 0, 0, 255, 0, 0, 255, 255]);
    let draw = canvas
        .push_image_with_sampling(
            Rect::new(10.0, 20.0, 14.0, 22.0),
            image,
            PatternSampling::Nearest,
        )
        .expect("push image draw");

    assert_eq!(draw.index(), 0);
    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    let record = &canvas.draw_records[0];
    assert_eq!(
        record.pixel_bounds,
        PixelBounds {
            x0: 10,
            y0: 20,
            x1: 14,
            y1: 22,
        }
    );
    assert!(matches!(record.sdf, Some(Sdf::Rect(_))));
    let Brush::Pattern(pattern) = &record.brush else {
        panic!("expected image pattern brush");
    };
    assert_eq!((pattern.image.width, pattern.image.height), (2, 1));
    assert_eq!(pattern.sampling, PatternSampling::Nearest);
    assert_eq!(pattern.transform, [0.5, 0.0, 0.0, 0.5, -5.0, -10.0]);
}

#[test]
fn append_translates_image_brush_without_mutating_child() {
    let mut child = test_scene();
    child
        .push_image(
            Rect::new(0.0, 0.0, 2.0, 1.0),
            Image::from_rgba8(2, 1, [255, 0, 0, 255, 0, 0, 255, 255]),
        )
        .expect("push child image");
    let Brush::Pattern(original_child_pattern) = &child.draw_records[0].brush else {
        panic!("expected child image pattern brush");
    };
    let original_transform = original_child_pattern.transform;

    let mut parent = test_scene();
    parent.append(&child, Point::new(10.0, 20.0));

    let Brush::Pattern(child_pattern) = &child.draw_records[0].brush else {
        panic!("expected child image pattern brush");
    };
    assert_eq!(child_pattern.transform, original_transform);
    let Brush::Pattern(parent_pattern) = &parent.draw_records[0].brush else {
        panic!("expected parent image pattern brush");
    };
    assert_eq!(parent_pattern.transform, [1.0, 0.0, 0.0, 1.0, -10.0, -20.0]);
    assert_eq!(
        parent.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 10,
            y0: 20,
            x1: 12,
            y1: 21,
        }
    );
}

#[test]
fn push_image_rejects_empty_images_and_rects() {
    let mut canvas = test_scene();

    assert!(
        canvas
            .push_image(Rect::new(0.0, 0.0, 4.0, 4.0), Image::from_rgba8(0, 1, []),)
            .is_none()
    );
    assert!(
        canvas
            .push_image(
                Rect::new(0.0, 0.0, 0.0, 4.0),
                Image::from_rgba8(1, 1, [255, 0, 0, 255]),
            )
            .is_none()
    );
    assert!(canvas.draw_records.is_empty());
}

#[test]
fn push_circle_records_sdf_circle_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_circle(Circle::new((16.0, 20.0), 8.0), Brush::Solid(rgb(255, 0, 0)));

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    let draw = &canvas.draw_records[0];
    assert_eq!(
        draw.pixel_bounds,
        PixelBounds {
            x0: 8,
            y0: 12,
            x1: 24,
            y1: 28,
        }
    );
    match draw.sdf {
        Some(Sdf::Circle(circle)) => {
            assert_eq!(circle.center, Point::new(16.0, 20.0));
            assert_eq!(circle.radius, 8.0);
        }
        sdf => panic!("expected circle SDF, got {sdf:?}"),
    }
}

#[test]
fn draw_id_updates_specific_draw_color() {
    let mut canvas = test_scene();
    let first = canvas.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    let second = canvas.push_circle(Circle::new((32.0, 32.0), 8.0), Brush::Solid(rgb(0, 255, 0)));

    assert_eq!(first.index(), 0);
    assert_eq!(second.index(), 1);
    assert_eq!(canvas.draw_id_at(0), Some(first));
    assert_eq!(canvas.draw_id_at(1), Some(second));

    assert!(canvas.set_draw_color(first, rgb(8, 9, 10)));
    assert_eq!(canvas.draw_solid_color(first), Some(rgb(8, 9, 10)));
    assert_eq!(canvas.draw_solid_color(second), Some(rgb(0, 255, 0)));
    assert_eq!(
        canvas.columns.draw_brush_colors[first.index()],
        premul_f32_to_u32(rgb(8, 9, 10).premultiply().components)
    );
    assert_eq!(
        canvas.columns.draw_brushes.data[first.index() * GPU_BRUSH_U32_STRIDE + 4],
        premul_color_to_rgba8_pack(rgb(8, 9, 10))
    );
    assert_eq!(
        canvas.columns.draw_brushes.data[second.index() * GPU_BRUSH_U32_STRIDE + 4],
        premul_color_to_rgba8_pack(rgb(0, 255, 0))
    );
}

#[test]
fn draw_id_from_before_reset_is_rejected() {
    let mut canvas = test_scene();
    let stale = canvas.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );

    canvas.reset();
    let current = canvas.push_rect(
        Rect::new(8.0, 9.0, 24.0, 25.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(0, 255, 0)),
    );

    assert_eq!(stale.index(), current.index());
    assert_ne!(stale, current);
    assert!(!canvas.set_draw_color(stale, rgb(255, 255, 0)));
    assert_eq!(canvas.draw_solid_color(current), Some(rgb(0, 255, 0)));
}

#[test]
fn no_op_sdf_primitive_returns_no_draw_id() {
    let mut canvas = test_scene();
    let draw = canvas.push_line(
        SdfLine::new(
            Point::new(8.0, 16.5),
            Point::new(8.0, 16.5),
            1.0,
            crate::shared::sdf::line::LineCap::Butt,
        ),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(draw, None);
    assert!(canvas.draw_records.is_empty());
    assert!(canvas.columns.draw_path_ids.is_empty());
}

#[test]
fn scene_columns_rebuild_after_append() {
    let mut parent = test_scene();
    parent.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );

    let mut child = test_scene();
    child.push_circle(Circle::new((16.0, 16.0), 8.0), Brush::Solid(rgb(0, 255, 0)));
    parent.append(&child, Point::new(4.0, 5.0));

    assert_eq!(
        parent.columns.draw_path_ids.len(),
        parent.draw_records.len()
    );
    assert_eq!(
        parent.columns.draw_brush_colors.len(),
        parent.draw_records.len()
    );
    assert_eq!(parent.columns.draw_flags.len(), parent.draw_records.len());
    assert_eq!(
        parent.columns.draw_flags_without_text.len(),
        parent.draw_records.len()
    );
    assert_eq!(parent.columns.sdf.refs.len(), parent.draw_records.len());
    assert_eq!(parent.columns.sdf.kinds.len(), 2);
}

#[test]
fn append_fast_path_translates_sdf_without_mutating_child() {
    let mut child = test_scene();
    child.push_rect(
        Rect::new(1.0, 2.0, 5.0, 6.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    let original_child_draw = child.draw_records[0].clone();

    let mut parent = test_scene();
    parent.append(&child, Point::new(10.0, 20.0));
    parent.append(&child, Point::new(30.0, 40.0));

    assert_eq!(
        child.draw_records[0].pixel_bounds,
        original_child_draw.pixel_bounds
    );
    assert!(matches!(child.draw_records[0].sdf, Some(Sdf::Rect(_))));
    assert!(matches!(original_child_draw.sdf, Some(Sdf::Rect(_))));
    assert_eq!(parent.draw_records.len(), 2);
    assert_eq!(
        parent.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 11,
            y0: 22,
            x1: 15,
            y1: 26,
        }
    );
    assert_eq!(
        parent.draw_records[1].pixel_bounds,
        PixelBounds {
            x0: 31,
            y0: 42,
            x1: 35,
            y1: 46,
        }
    );
    assert_eq!(parent.columns.draw_flags.len(), parent.draw_records.len());
    assert_eq!(parent.columns.sdf.refs.len(), parent.draw_records.len());
    assert_eq!(parent.columns.sdf.kinds.len(), parent.draw_records.len());
}

#[test]
fn append_fast_path_offsets_text_runs_without_mutating_child() {
    let mut context = TextContext::new();
    let layout = context.layout(TextLayoutOptions::new("AA", 20.0));
    if layout.is_empty() {
        return;
    }

    let mut child = test_scene();
    child
        .push_text_layout(&layout, Point::new(0.0, 20.0), Brush::Solid(rgb(0, 0, 0)))
        .expect("layout should produce a text draw");
    let child_runs = child.text_runs.clone();
    let child_glyphs = child.text_glyphs.clone();

    let mut parent = test_scene();
    parent.append(&child, Point::new(4.0, 8.0));
    parent.append(&child, Point::new(40.0, 80.0));

    assert_eq!(child.text_runs.len(), child_runs.len());
    for (actual, expected) in child.text_runs.iter().zip(&child_runs) {
        assert_eq!(actual.glyph_start, expected.glyph_start);
        assert_eq!(actual.glyph_count, expected.glyph_count);
    }
    assert_eq!(child.text_glyphs.len(), child_glyphs.len());
    for (actual, expected) in child.text_glyphs.iter().zip(&child_glyphs) {
        assert_eq!(actual.x, expected.x);
        assert_eq!(actual.y, expected.y);
    }
    assert_eq!(parent.text_runs.len(), 2);
    assert_eq!(parent.text_runs[0].glyph_start, 0);
    assert_eq!(parent.text_runs[1].glyph_start, child_glyphs.len() as u32);
    assert_eq!(
        parent.columns.text_run_starts,
        vec![0, child_glyphs.len() as u32]
    );
    assert_eq!(parent.columns.glyph_x.len(), parent.text_glyphs.len());
    assert_eq!(parent.columns.glyph_y.len(), parent.text_glyphs.len());
    assert_eq!(parent.columns.draw_glyph_run_ids[0], 0);
    assert_eq!(parent.columns.draw_glyph_run_ids[1], 1);
}

#[test]
fn scene_columns_track_text_runs_and_glyph_positions() {
    let mut context = TextContext::new();
    let layout = context.layout(TextLayoutOptions::new("Cache", 20.0));
    if layout.is_empty() {
        return;
    }

    let mut canvas = test_scene();
    let draw = canvas
        .push_text_layout(&layout, Point::new(8.0, 32.0), Brush::Solid(rgb(0, 0, 0)))
        .expect("layout should produce a text draw");

    assert_eq!(canvas.columns.draw_glyph_run_ids[draw.index()], 0);
    assert_eq!(canvas.columns.text_run_starts, vec![0]);
    assert_eq!(
        canvas.columns.text_run_counts,
        vec![canvas.text_glyphs.len() as u32]
    );
    assert_eq!(canvas.columns.glyph_x.len(), canvas.text_glyphs.len());
    assert_eq!(canvas.columns.glyph_y.len(), canvas.text_glyphs.len());
}

#[test]
fn push_candlestick_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_candlestick(
        SdfCandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 7, 1),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 13,
            y0: 4,
            x1: 20,
            y1: 28,
        }
    );
    match canvas.draw_records[0].sdf {
        Some(Sdf::CandleStick(candle)) => {
            assert_eq!(candle.center_x, 16.5);
            assert_eq!(candle.body_width, 7);
            assert_eq!(candle.wick_width, 1);
        }
        sdf => panic!("expected candlestick SDF, got {sdf:?}"),
    }
}

#[test]
fn push_candlestick_accepts_even_body_and_custom_wick_width_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_candlestick(
        SdfCandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 8, 3),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 12,
            y0: 4,
            x1: 21,
            y1: 28,
        }
    );
    match canvas.draw_records[0].sdf {
        Some(Sdf::CandleStick(candle)) => {
            assert_eq!(candle.body_width, 8);
            assert_eq!(candle.wick_width, 3);
        }
        sdf => panic!("expected candlestick SDF, got {sdf:?}"),
    }
}

#[test]
fn push_line_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_line(
        SdfLine::new(
            Point::new(8.0, 16.5),
            Point::new(24.0, 16.5),
            1.0,
            crate::shared::sdf::line::LineCap::Butt,
        ),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 7,
            y0: 16,
            x1: 25,
            y1: 17,
        }
    );
    match canvas.draw_records[0].sdf {
        Some(Sdf::Line(line)) => assert_eq!(line.width, 1.0),
        sdf => panic!("expected line SDF, got {sdf:?}"),
    }
}

#[test]
fn push_dash_line_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_dash_line(
        crate::SdfDashLine::new(
            Point::new(8.0, 16.5),
            Point::new(32.0, 16.5),
            1.0,
            crate::shared::sdf::line::LineCap::Butt,
            4.0,
            3.0,
        ),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 7,
            y0: 16,
            x1: 33,
            y1: 17,
        }
    );
    match canvas.draw_records[0].sdf {
        Some(Sdf::DashLine(line)) => {
            assert_eq!(line.dash_length, 4.0);
            assert_eq!(line.gap_length, 3.0);
        }
        sdf => panic!("expected dash line SDF, got {sdf:?}"),
    }
}

#[test]
fn push_sdf_arc_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_sdf_arc(
        SdfArc::new(
            Point::new(32.0, 32.0),
            12.0,
            0.0,
            std::f32::consts::FRAC_PI_2,
            4.0,
            crate::shared::sdf::line::LineCap::Round,
        ),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    match canvas.draw_records[0].sdf {
        Some(Sdf::Arc(arc)) => {
            assert_eq!(arc.center, Point::new(32.0, 32.0));
            assert_eq!(arc.radius, 12.0);
            assert_eq!(arc.width, 4.0);
        }
        sdf => panic!("expected arc SDF, got {sdf:?}"),
    }
}

#[test]
fn push_shape_shadows_record_sdf_shadow_without_path_storage() {
    let mut canvas = test_scene();
    let options = RectShadowOptions::new(2.0, 3.0, 4.0, 0.5);
    canvas.push_circle_shadow(
        Circle::new((20.0, 20.0), 8.0),
        options,
        Brush::Solid(rgb(0, 0, 0)),
    );
    canvas.push_arc_shadow(
        SdfArc::new(
            Point::new(32.0, 32.0),
            10.0,
            0.0,
            std::f32::consts::FRAC_PI_2,
            3.0,
            crate::shared::sdf::line::LineCap::Round,
        ),
        options,
        Brush::Solid(rgb(0, 0, 0)),
    );
    canvas.push_line_shadow(
        SdfLine::new(
            Point::new(8.0, 12.0),
            Point::new(28.0, 12.0),
            2.0,
            crate::shared::sdf::line::LineCap::Butt,
        ),
        options,
        Brush::Solid(rgb(0, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 3);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    assert!(canvas.draw_records.iter().all(|draw| draw.sdf.is_none()));
    assert!(matches!(
        canvas.draw_records[0].sdf_shadow,
        Some(SdfShadow::Circle(_))
    ));
    assert!(matches!(
        canvas.draw_records[1].sdf_shadow,
        Some(SdfShadow::Arc(_))
    ));
    assert!(matches!(
        canvas.draw_records[2].sdf_shadow,
        Some(SdfShadow::Line(_))
    ));
}

#[test]
fn push_rect_stroke_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_rect_stroke(
        Rect::new(10.0, 12.0, 30.0, 36.0),
        Radius::all(4.0),
        Stroke::new(6.0),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    let draw = &canvas.draw_records[0];
    assert_eq!(
        draw.pixel_bounds,
        PixelBounds {
            x0: 7,
            y0: 9,
            x1: 33,
            y1: 39,
        }
    );
    assert!(draw.path_id.is_none());
    match draw.sdf {
        Some(Sdf::RectStroke(stroke)) => {
            assert_eq!(stroke.rect.axis_bounds(), (10.0, 12.0, 30.0, 36.0));
            assert_eq!(stroke.rect.radius.top_left, 4.0);
            assert_eq!(stroke.widths, StrokeWidths::all(6.0));
        }
        sdf => panic!("expected rect stroke SDF, got {sdf:?}"),
    }
}

#[test]
fn push_rect_stroke_widths_records_per_side_sdf_widths() {
    let mut canvas = test_scene();
    let widths = StrokeWidths {
        top: 2.0,
        right: 6.0,
        bottom: 10.0,
        left: 4.0,
    };
    canvas.push_rect_stroke_widths(
        Rect::new(10.0, 12.0, 30.0, 36.0),
        Radius::all(4.0),
        widths,
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    let draw = &canvas.draw_records[0];
    assert_eq!(
        draw.pixel_bounds,
        PixelBounds {
            x0: 8,
            y0: 11,
            x1: 33,
            y1: 41,
        }
    );
    match draw.sdf {
        Some(Sdf::RectStroke(stroke)) => {
            assert_eq!(stroke.rect.axis_bounds(), (10.0, 12.0, 30.0, 36.0));
            assert_eq!(stroke.widths, widths);
        }
        sdf => panic!("expected rect stroke SDF, got {sdf:?}"),
    }
}

#[test]
fn push_circle_stroke_records_sdf_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_circle_stroke(
        Circle::new((24.0, 20.0), 10.0),
        Stroke::new(4.0),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
    let draw = &canvas.draw_records[0];
    assert_eq!(
        draw.pixel_bounds,
        PixelBounds {
            x0: 12,
            y0: 8,
            x1: 36,
            y1: 32,
        }
    );
    match draw.sdf {
        Some(Sdf::CircleStroke(stroke)) => {
            assert_eq!(stroke.circle.center, Point::new(24.0, 20.0));
            assert_eq!(stroke.circle.radius, 10.0);
            assert_eq!(stroke.half_width, 2.0);
        }
        sdf => panic!("expected circle stroke SDF, got {sdf:?}"),
    }
}

#[test]
fn push_sdf_stroke_with_zero_width_is_noop() {
    let mut canvas = test_scene();
    canvas.push_rect_stroke(
        Rect::new(10.0, 12.0, 30.0, 36.0),
        Radius::ZERO,
        Stroke::new(0.0),
        Brush::Solid(rgb(255, 0, 0)),
    );
    canvas.push_circle_stroke(
        Circle::new((24.0, 20.0), 10.0),
        Stroke::new(0.0),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert!(canvas.draw_records.is_empty());
    assert!(canvas.path_records.is_empty());
    assert!(canvas.bd_records.is_empty());
}

#[test]
fn push_dashed_circle_stroke_uses_path_storage() {
    let mut canvas = test_scene();
    canvas.push_circle_stroke(
        Circle::new((24.0, 20.0), 10.0),
        Stroke::new(4.0).with_dashes(0.0, [4.0, 4.0]),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert_eq!(canvas.path_records.len(), 1);
    assert_eq!(canvas.bd_records.len(), 1);
    assert!(canvas.draw_records[0].path_id.is_some());
    assert!(canvas.draw_records[0].sdf.is_none());
    assert!(canvas.draw_records[0].sdf_shadow.is_none());
}
