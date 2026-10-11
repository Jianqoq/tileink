use super::*;

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
    assert!(draw.path_id().is_none());
    assert!(!draw.solid_rect());
    match canvas.draw_sdf(draw) {
        Some(Sdf::Rect(rect)) => {
            assert_eq!(rect.axis_bounds(), (2.0, 3.0, 18.0, 19.0));
            assert!(rect.radius.is_zero());
        }
        sdf => panic!("expected rect SDF, got {sdf:?}"),
    }
}

#[test]
fn push_checkerboard_records_two_sdf_draws_independent_of_area() {
    let mut canvas = test_scene();
    let draws = canvas
        .push_checkerboard(
            Rect::new(2.0, 3.0, 18.0, 35.0),
            4.0,
            rgb(194, 194, 194),
            rgb(255, 255, 255),
        )
        .unwrap();

    assert_eq!((draws.0.index(), draws.1.index()), (0, 1));
    assert_eq!(canvas.draw_count(), 2);
    assert!(canvas.path_records.is_empty());
    assert!(matches!(draw_sdf(&canvas, 0), Some(Sdf::Rect(_))));
    match draw_sdf(&canvas, 1) {
        Some(Sdf::Checkerboard(checkerboard)) => {
            assert_eq!(checkerboard.axis_bounds(), (2.0, 3.0, 18.0, 35.0));
            assert_eq!(checkerboard.cell_size, 4.0);
        }
        sdf => panic!("expected checkerboard SDF, got {sdf:?}"),
    }
}

#[test]
fn push_checkerboard_scales_geometry_and_cell_size_to_physical_pixels() {
    let mut canvas = Canvas::new(32, 32, 2.0);
    canvas
        .push_checkerboard(
            Rect::new(1.0, 2.0, 9.0, 10.0),
            3.0,
            Color::BLACK,
            Color::WHITE,
        )
        .unwrap();

    match draw_sdf(&canvas, 1) {
        Some(Sdf::Checkerboard(checkerboard)) => {
            assert_eq!(checkerboard.axis_bounds(), (2.0, 4.0, 18.0, 20.0));
            assert_eq!(checkerboard.cell_size, 6.0);
        }
        sdf => panic!("expected scaled checkerboard SDF, got {sdf:?}"),
    }
}

#[test]
fn push_checkerboard_rejects_invalid_inputs_without_partial_draws() {
    let mut canvas = test_scene();
    assert!(
        canvas
            .push_checkerboard(
                Rect::new(0.0, 0.0, 16.0, 16.0),
                f32::NAN,
                Color::BLACK,
                Color::WHITE,
            )
            .is_none()
    );
    assert!(
        canvas
            .push_checkerboard(
                Rect::new(0.0, 0.0, 0.0, 16.0),
                4.0,
                Color::BLACK,
                Color::WHITE,
            )
            .is_none()
    );
    assert_eq!(canvas.draw_count(), 0);
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
    match draw_sdf(&canvas, 0) {
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
fn new_stores_logical_size_and_scales_sdf_rects() {
    let mut canvas = Canvas::new(21, 11, 1.5);
    assert_eq!(canvas.physical_size(), (32, 17));
    assert_eq!(canvas.scale_factor(), 1.5);

    canvas.push_rect(
        Rect::new(2.0, 3.0, 10.0, 11.0),
        Radius {
            top_left: 1.0,
            top_right: 2.0,
            bottom_left: 3.0,
            bottom_right: 4.0,
        },
        Brush::Solid(rgb(255, 0, 0)),
    );

    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 3,
            y0: 4,
            x1: 15,
            y1: 17,
        }
    );
    match draw_sdf(&canvas, 0) {
        Some(Sdf::Rect(rect)) => {
            assert_eq!(rect.axis_bounds(), (3.0, 4.5, 15.0, 16.5));
            assert_eq!(rect.radius.top_left, 1.5);
            assert_eq!(rect.radius.top_right, 3.0);
            assert_eq!(rect.radius.bottom_left, 4.5);
            assert_eq!(rect.radius.bottom_right, 6.0);
        }
        sdf => panic!("expected scaled rect SDF, got {sdf:?}"),
    }
}
