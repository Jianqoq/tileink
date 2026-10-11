use super::*;

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
    assert!(
        canvas
            .draw_records
            .iter()
            .all(|draw| draw.sdf_range().is_none())
    );
    assert!(matches!(
        draw_sdf_shadow(&canvas, 0),
        Some(SdfShadow::Circle(_))
    ));
    assert!(matches!(
        draw_sdf_shadow(&canvas, 1),
        Some(SdfShadow::Rc(_))
    ));
    assert!(matches!(
        draw_sdf_shadow(&canvas, 2),
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
    assert!(draw.path_id().is_none());
    match canvas.draw_sdf(draw) {
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
    match canvas.draw_sdf(draw) {
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
    match canvas.draw_sdf(draw) {
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
    assert!(canvas.draw_records[0].path_id().is_some());
    assert!(canvas.draw_records[0].sdf_range().is_none());
    assert!(canvas.draw_records[0].sdf_shadow_range().is_none());
}
