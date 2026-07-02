use super::*;
use crate::{
    TextLayoutOptions,
    cubecl::scene_columns::GPU_BRUSH_U32_STRIDE,
    shared::{image::premul_color_to_rgba8_pack, pixel::premul_f32_to_u32},
};

#[test]
fn push_rect_records_sdf_rect_without_path_storage() {
    let mut scene = test_scene();
    scene.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(scene.draw_records.len(), 1);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    let draw = &scene.draw_records[0];
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
    let mut scene = test_scene();
    let radius = Radius {
        top_left: 3.0,
        top_right: 9.0,
        bottom_left: 15.0,
        bottom_right: 21.0,
    };
    scene.push_rect(
        Rect::new(4.0, 5.0, 40.0, 41.0),
        radius,
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(scene.draw_records.len(), 1);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    match scene.draw_records[0].sdf {
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
fn push_circle_records_sdf_circle_without_path_storage() {
    let mut scene = test_scene();
    scene.push_circle(Circle::new((16.0, 20.0), 8.0), Brush::Solid(rgb(255, 0, 0)));

    assert_eq!(scene.draw_records.len(), 1);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    let draw = &scene.draw_records[0];
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
    let mut scene = test_scene();
    let first = scene.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    let second = scene.push_circle(Circle::new((32.0, 32.0), 8.0), Brush::Solid(rgb(0, 255, 0)));

    assert_eq!(first.index(), 0);
    assert_eq!(second.index(), 1);
    assert_eq!(scene.draw_id_at(0), Some(first));
    assert_eq!(scene.draw_id_at(1), Some(second));

    assert!(scene.set_draw_color(first, rgb(8, 9, 10)));
    assert_eq!(scene.draw_solid_color(first), Some(rgb(8, 9, 10)));
    assert_eq!(scene.draw_solid_color(second), Some(rgb(0, 255, 0)));
    assert_eq!(
        scene.columns.draw_brush_colors[first.index()],
        premul_f32_to_u32(rgb(8, 9, 10).premultiply().components)
    );
    assert_eq!(
        scene.columns.draw_brushes.data[first.index() * GPU_BRUSH_U32_STRIDE + 4],
        premul_color_to_rgba8_pack(rgb(8, 9, 10))
    );
    assert_eq!(
        scene.columns.draw_brushes.data[second.index() * GPU_BRUSH_U32_STRIDE + 4],
        premul_color_to_rgba8_pack(rgb(0, 255, 0))
    );
}

#[test]
fn draw_id_from_before_reset_is_rejected() {
    let mut scene = test_scene();
    let stale = scene.push_rect(
        Rect::new(2.0, 3.0, 18.0, 19.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );

    scene.reset();
    let current = scene.push_rect(
        Rect::new(8.0, 9.0, 24.0, 25.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(0, 255, 0)),
    );

    assert_eq!(stale.index(), current.index());
    assert_ne!(stale, current);
    assert!(!scene.set_draw_color(stale, rgb(255, 255, 0)));
    assert_eq!(scene.draw_solid_color(current), Some(rgb(0, 255, 0)));
}

#[test]
fn no_op_sdf_primitive_returns_no_draw_id() {
    let mut scene = test_scene();
    let draw = scene.push_line(
        SdfLine::new(
            Point::new(8.0, 16.5),
            Point::new(8.0, 16.5),
            1.0,
            crate::shared::sdf::line::LineCap::Butt,
        ),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(draw, None);
    assert!(scene.draw_records.is_empty());
    assert!(scene.columns.draw_path_ids.is_empty());
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
    parent.append(child, Point::new(4.0, 5.0));

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
fn scene_columns_track_text_runs_and_glyph_positions() {
    let mut context = TextContext::new();
    let layout = context.layout(TextLayoutOptions::new("Cache", 20.0));
    if layout.is_empty() {
        return;
    }

    let mut scene = test_scene();
    let draw = scene
        .push_text_layout(&layout, Point::new(8.0, 32.0), Brush::Solid(rgb(0, 0, 0)))
        .expect("layout should produce a text draw");

    assert_eq!(scene.columns.draw_glyph_run_ids[draw.index()], 0);
    assert_eq!(scene.columns.text_run_starts, vec![0]);
    assert_eq!(
        scene.columns.text_run_counts,
        vec![scene.text_glyphs.len() as u32]
    );
    assert_eq!(scene.columns.glyph_x.len(), scene.text_glyphs.len());
    assert_eq!(scene.columns.glyph_y.len(), scene.text_glyphs.len());
}

#[test]
fn push_candlestick_records_sdf_without_path_storage() {
    let mut scene = test_scene();
    scene.push_candlestick(
        SdfCandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 7),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(scene.draw_records.len(), 1);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    assert_eq!(
        scene.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 13,
            y0: 4,
            x1: 20,
            y1: 28,
        }
    );
    match scene.draw_records[0].sdf {
        Some(Sdf::CandleStick(candle)) => {
            assert_eq!(candle.center_x, 16.5);
            assert_eq!(candle.body_width, 7);
        }
        sdf => panic!("expected candlestick SDF, got {sdf:?}"),
    }
}

#[test]
fn push_line_records_sdf_without_path_storage() {
    let mut scene = test_scene();
    scene.push_line(
        SdfLine::new(
            Point::new(8.0, 16.5),
            Point::new(24.0, 16.5),
            1.0,
            crate::shared::sdf::line::LineCap::Butt,
        ),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(scene.draw_records.len(), 1);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    assert_eq!(
        scene.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 7,
            y0: 16,
            x1: 25,
            y1: 17,
        }
    );
    match scene.draw_records[0].sdf {
        Some(Sdf::Line(line)) => assert_eq!(line.width, 1.0),
        sdf => panic!("expected line SDF, got {sdf:?}"),
    }
}

#[test]
fn push_dash_line_records_sdf_without_path_storage() {
    let mut scene = test_scene();
    scene.push_dash_line(
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

    assert_eq!(scene.draw_records.len(), 1);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    assert_eq!(
        scene.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 7,
            y0: 16,
            x1: 33,
            y1: 17,
        }
    );
    match scene.draw_records[0].sdf {
        Some(Sdf::DashLine(line)) => {
            assert_eq!(line.dash_length, 4.0);
            assert_eq!(line.gap_length, 3.0);
        }
        sdf => panic!("expected dash line SDF, got {sdf:?}"),
    }
}

#[test]
fn push_sdf_arc_records_sdf_without_path_storage() {
    let mut scene = test_scene();
    scene.push_sdf_arc(
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

    assert_eq!(scene.draw_records.len(), 1);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    match scene.draw_records[0].sdf {
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
    let mut scene = test_scene();
    let options = RectShadowOptions::new(2.0, 3.0, 4.0, 0.5);
    scene.push_circle_shadow(
        Circle::new((20.0, 20.0), 8.0),
        options,
        Brush::Solid(rgb(0, 0, 0)),
    );
    scene.push_arc_shadow(
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
    scene.push_line_shadow(
        SdfLine::new(
            Point::new(8.0, 12.0),
            Point::new(28.0, 12.0),
            2.0,
            crate::shared::sdf::line::LineCap::Butt,
        ),
        options,
        Brush::Solid(rgb(0, 0, 0)),
    );

    assert_eq!(scene.draw_records.len(), 3);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    assert!(scene.draw_records.iter().all(|draw| draw.sdf.is_none()));
    assert!(matches!(
        scene.draw_records[0].sdf_shadow,
        Some(SdfShadow::Circle(_))
    ));
    assert!(matches!(
        scene.draw_records[1].sdf_shadow,
        Some(SdfShadow::Arc(_))
    ));
    assert!(matches!(
        scene.draw_records[2].sdf_shadow,
        Some(SdfShadow::Line(_))
    ));
}

#[test]
fn push_rect_stroke_records_sdf_without_path_storage() {
    let mut scene = test_scene();
    scene.push_rect_stroke(
        Rect::new(10.0, 12.0, 30.0, 36.0),
        Radius::all(4.0),
        Stroke::new(6.0),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(scene.draw_records.len(), 1);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    let draw = &scene.draw_records[0];
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
    let mut scene = test_scene();
    let widths = StrokeWidths {
        top: 2.0,
        right: 6.0,
        bottom: 10.0,
        left: 4.0,
    };
    scene.push_rect_stroke_widths(
        Rect::new(10.0, 12.0, 30.0, 36.0),
        Radius::all(4.0),
        widths,
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(scene.draw_records.len(), 1);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    let draw = &scene.draw_records[0];
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
    let mut scene = test_scene();
    scene.push_circle_stroke(
        Circle::new((24.0, 20.0), 10.0),
        Stroke::new(4.0),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(scene.draw_records.len(), 1);
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
    let draw = &scene.draw_records[0];
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
    let mut scene = test_scene();
    scene.push_rect_stroke(
        Rect::new(10.0, 12.0, 30.0, 36.0),
        Radius::ZERO,
        Stroke::new(0.0),
        Brush::Solid(rgb(255, 0, 0)),
    );
    scene.push_circle_stroke(
        Circle::new((24.0, 20.0), 10.0),
        Stroke::new(0.0),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert!(scene.draw_records.is_empty());
    assert!(scene.path_records.is_empty());
    assert!(scene.bd_records.is_empty());
}

#[test]
fn push_dashed_circle_stroke_uses_path_storage() {
    let mut scene = test_scene();
    scene.push_circle_stroke(
        Circle::new((24.0, 20.0), 10.0),
        Stroke::new(4.0).with_dashes(0.0, [4.0, 4.0]),
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(scene.draw_records.len(), 1);
    assert_eq!(scene.path_records.len(), 1);
    assert_eq!(scene.bd_records.len(), 1);
    assert!(scene.draw_records[0].path_id.is_some());
    assert!(scene.draw_records[0].sdf.is_none());
    assert!(scene.draw_records[0].sdf_shadow.is_none());
}
