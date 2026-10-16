use super::*;

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
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 13,
            y0: 4,
            x1: 20,
            y1: 28,
        }
    );
    match draw_sdf(&canvas, 0) {
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
    match draw_sdf(&canvas, 0) {
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
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 7,
            y0: 16,
            x1: 25,
            y1: 17,
        }
    );
    match draw_sdf(&canvas, 0) {
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
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 7,
            y0: 16,
            x1: 33,
            y1: 17,
        }
    );
    match draw_sdf(&canvas, 0) {
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
    match draw_sdf(&canvas, 0) {
        Some(Sdf::Rc(arc)) => {
            assert_eq!(arc.center, Point::new(32.0, 32.0));
            assert_eq!(arc.radius, 12.0);
            assert_eq!(arc.width, 4.0);
        }
        sdf => panic!("expected arc SDF, got {sdf:?}"),
    }
}
