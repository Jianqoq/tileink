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
