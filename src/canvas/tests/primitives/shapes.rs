use super::*;

#[test]
fn push_triangle_records_rounded_sdf_without_path_storage() {
    let mut canvas = test_scene();
    let triangle = crate::SdfTriangle::new(
        Point::new(8.0, 4.0),
        Point::new(20.0, 16.0),
        Point::new(8.0, 28.0),
        2.0,
    );

    canvas.push_triangle(triangle, Brush::Solid(rgb(255, 0, 0)));

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 6,
            y0: 2,
            x1: 22,
            y1: 30,
        }
    );
    match draw_sdf(&canvas, 0) {
        Some(Sdf::Triangle(actual)) => assert_eq!(actual, triangle),
        sdf => panic!("expected triangle SDF, got {sdf:?}"),
    }
}

#[test]
fn push_triangle_scales_points_radius_and_bounds_to_physical_pixels() {
    let mut canvas = Canvas::new(64, 64, 2.0);
    let logical = crate::SdfTriangle::new(
        Point::new(8.0, 4.0),
        Point::new(20.0, 16.0),
        Point::new(8.0, 28.0),
        2.0,
    );

    canvas.push_triangle(logical, Brush::Solid(rgb(255, 0, 0)));

    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 12,
            y0: 4,
            x1: 44,
            y1: 60,
        }
    );
    match draw_sdf(&canvas, 0) {
        Some(Sdf::Triangle(physical)) => {
            assert_eq!(physical.a, Point::new(16.0, 8.0));
            assert_eq!(physical.b, Point::new(40.0, 32.0));
            assert_eq!(physical.c, Point::new(16.0, 56.0));
            assert_eq!(physical.corner_radius, 4.0);
        }
        sdf => panic!("expected triangle SDF, got {sdf:?}"),
    }
}

#[test]
fn push_triangle_rejects_invalid_public_struct_literals_without_mutating_canvas() {
    let mut canvas = test_scene();
    let invalid = crate::SdfTriangle {
        a: Point::new(0.0, 0.0),
        b: Point::new(1.0, 1.0),
        c: Point::new(2.0, 2.0),
        corner_radius: 0.0,
    };

    assert_eq!(canvas.push_triangle(invalid, Color::WHITE), None);
    assert_eq!(canvas.draw_count(), 0);
}

#[test]
fn push_callout_records_unified_fill_stroke_and_shadow_without_paths() {
    let mut canvas = test_scene();
    let callout = crate::SdfCallout::new(
        Rect::new(10.0, 20.0, 110.0, 60.0),
        8.0,
        crate::SdfCalloutTail::new(crate::SdfCalloutSide::Bottom, 50.0, 10.0, 6.0, 1.5),
    );
    let stroke = crate::SdfCalloutStroke::new(callout, 0.5);
    let shadow = crate::ShadowOptions::new(0.0, 6.0, 4.0, 0.28);

    canvas
        .push_callout_shadow(callout, shadow, Color::BLACK)
        .unwrap();
    canvas.push_callout(callout, Color::BLACK).unwrap();
    canvas.push_callout_stroke(stroke, Color::WHITE).unwrap();

    assert_eq!(canvas.draw_count(), 3);
    assert!(canvas.path_records.is_empty());
    assert!(
        matches!(draw_sdf_shadow(&canvas, 0), Some(SdfShadow::Callout(value)) if value.callout == callout)
    );
    assert!(matches!(draw_sdf(&canvas, 1), Some(Sdf::Callout(value)) if value == callout));
    assert!(matches!(draw_sdf(&canvas, 2), Some(Sdf::CalloutStroke(value)) if value == stroke));
}

#[test]
fn push_callout_scales_tail_geometry_and_can_hide_it() {
    let mut canvas = Canvas::new(128, 128, 2.0);
    let callout = crate::SdfCallout::new(
        Rect::new(10.0, 20.0, 110.0, 60.0),
        8.0,
        crate::SdfCalloutTail::hidden(),
    );

    canvas.push_callout(callout, Color::BLACK).unwrap();

    let Some(Sdf::Callout(physical)) = draw_sdf(&canvas, 0) else {
        panic!("expected scaled callout");
    };
    assert_eq!(physical.start, Point::new(20.0, 40.0));
    assert_eq!(physical.end, Point::new(220.0, 120.0));
    assert_eq!(physical.body_radius, 16.0);
    assert!(!physical.tail.visible);
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 20,
            y0: 40,
            x1: 220,
            y1: 120,
        }
    );
}

#[test]
fn push_star_fill_and_stroke_each_record_one_sdf_draw() {
    let mut canvas = test_scene();
    let star = crate::SdfStar::new(Point::new(20.0, 20.0), 8.0, 3.5, 1.0, 0.0);

    canvas.push_star(star, Color::WHITE).unwrap();
    canvas
        .push_star_stroke(crate::SdfStarStroke::new(star, 2.5), Color::BLACK)
        .unwrap();

    assert_eq!(canvas.draw_count(), 2);
    assert!(canvas.path_records.is_empty());
    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 12,
            y0: 11,
            x1: 29,
            y1: 29,
        }
    );
    assert_eq!(
        canvas.draw_records[1].pixel_bounds,
        PixelBounds {
            x0: 11,
            y0: 10,
            x1: 31,
            y1: 30,
        }
    );
    assert!(matches!(draw_sdf(&canvas, 0), Some(Sdf::Star(value)) if value == star));
    assert!(
        matches!(draw_sdf(&canvas, 1), Some(Sdf::StarStroke(value)) if value == crate::SdfStarStroke::new(star, 2.5))
    );
}

#[test]
fn push_star_scales_radii_stroke_and_center_but_preserves_rotation() {
    let mut canvas = Canvas::new(64, 64, 2.0);
    let star = crate::SdfStar::new(
        Point::new(12.0, 14.0),
        7.0,
        3.0,
        1.25,
        -std::f32::consts::FRAC_PI_2,
    );

    canvas
        .push_star_stroke(crate::SdfStarStroke::new(star, 2.0), Color::WHITE)
        .unwrap();

    let Some(Sdf::StarStroke(stroke)) = draw_sdf(&canvas, 0) else {
        panic!("expected scaled star stroke");
    };
    assert_eq!(stroke.star.center, Point::new(24.0, 28.0));
    assert_eq!(stroke.star.outer_radius, 14.0);
    assert_eq!(stroke.star.inner_radius, 6.0);
    assert_eq!(stroke.star.corner_radius, 2.5);
    assert_eq!(stroke.star.rotation_radians, star.rotation_radians);
    assert_eq!(stroke.half_width, 2.0);
}

#[test]
fn push_star_rejects_invalid_public_literals_without_mutating_canvas() {
    let mut canvas = test_scene();
    let invalid = crate::SdfStar {
        center: Point::ZERO,
        outer_radius: 8.0,
        inner_radius: 9.0,
        corner_radius: 0.0,
        rotation_radians: 0.0,
    };

    assert_eq!(canvas.push_star(invalid, Color::WHITE), None);
    assert_eq!(
        canvas.push_star_stroke(
            crate::SdfStarStroke {
                star: crate::SdfStar {
                    inner_radius: 3.0,
                    ..invalid
                },
                half_width: f32::NAN,
            },
            Color::WHITE,
        ),
        None
    );
    assert_eq!(canvas.draw_count(), 0);
}

#[test]
fn push_circle_records_sdf_circle_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_circle(Circle::new((16.0, 20.0), 8.0), Brush::Solid(rgb(255, 0, 0)));

    assert_eq!(canvas.draw_records.len(), 1);
    assert!(canvas.path_records.is_empty());
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
    match canvas.draw_sdf(draw) {
        Some(Sdf::Circle(circle)) => {
            assert_eq!(circle.center, Point::new(16.0, 20.0));
            assert_eq!(circle.radius, 8.0);
        }
        sdf => panic!("expected circle SDF, got {sdf:?}"),
    }
}
