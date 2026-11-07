use super::*;

#[test]
fn approximate_closing_point_does_not_expand_the_rectangular_clip() {
    // A tolerated duplicate endpoint closes the first corner; it is not a fifth corner.
    for closing in [Point::new(-1.0e-11, 0.0), Point::new(0.0, -1.0e-11)] {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((32.0, 0.0));
        path.line_to((32.0, 32.0));
        path.line_to((0.0, 32.0));
        path.line_to(closing);
        path.close_path();
        assert_eq!(
            axis_aligned_rect_path(&path, Affine::IDENTITY),
            Some(Rect::new(0.0, 0.0, 32.0, 32.0))
        );
    }
}

#[test]
fn opacity_groups_allocate_no_geometry_and_preserve_overflow_bounds() {
    use crate::shared::execution::{ExecOp, OffscreenLayer};
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_opacity_group(0.5);
    assert_eq!(canvas.draw_count(), 0);
    canvas.push_rect(Rect::new(-4.0, 8.0, 48.0, 40.0), Radius::ZERO, Color::WHITE);
    assert_eq!(canvas.pop_layer(), Some(LayerKind::Opacity));
    let bounds = canvas.visual_bounds();
    assert!(bounds.x0 <= -4 && bounds.y0 <= 8 && bounds.x1 >= 48 && bounds.y1 >= 40);
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    assert!(matches!(
        &plan.ops[0],
        ExecOp::OffscreenLayer {
            layer: OffscreenLayer::Opacity {
                draw: None,
                opacity: 0.5
            },
            ..
        }
    ));
}

#[test]
#[should_panic(expected = "opacity must be finite")]
fn opacity_group_rejects_nonfinite_alpha() {
    Canvas::new(64, 64, 1.0).push_opacity_group(f32::NAN);
}

#[test]
fn blur_groups_resolve_descendant_bounds_and_expand_overflow_at_physical_dpi() {
    use crate::shared::execution::{ExecOp, OffscreenLayer};
    for dpi in [1.0, 1.25, 2.0] {
        let mut canvas = Canvas::new(64, 64, dpi);
        canvas.push_blur_group(2.0);
        canvas.push_rect(
            Rect::new(-10.0, 8.0, 90.0, 40.0),
            Radius::ZERO,
            Color::WHITE,
        );
        assert_eq!(canvas.pop_layer(), Some(LayerKind::Filter));
        assert_eq!(canvas.draw_count(), 1);
        let bounds = canvas.visual_bounds();
        assert!(f64::from(bounds.x0) < -10.0 * f64::from(dpi));
        assert!(f64::from(bounds.x1) > 90.0 * f64::from(dpi));
        let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let ExecOp::OffscreenLayer {
            layer:
                OffscreenLayer::Filter {
                    filter:
                        crate::Filter::Blur {
                            std_dev_x,
                            std_dev_y,
                            ..
                        },
                    sample_region,
                },
            ..
        } = &plan.ops[0]
        else {
            panic!("real blur layer");
        };
        assert_eq!((*std_dev_x, *std_dev_y), (2.0 * dpi, 2.0 * dpi));
        assert!(
            crate::shared::layer::filter::region_bounds(sample_region).x0 <= (-10.0 * dpi) as i32
        );
    }
}
#[test]
fn nested_blur_groups_are_finite_and_empty_groups_stay_empty() {
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_blur_group(2.0);
    canvas.push_blur_group(3.0);
    canvas.pop_layer();
    canvas.pop_layer();
    assert!(canvas.visual_bounds().is_empty());
}
