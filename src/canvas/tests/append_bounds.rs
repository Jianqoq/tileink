use super::*;

#[test]
fn translated_path_bounds_remain_local_to_the_appended_geometry() {
    for scale in [1.0, 2.0] {
        for offset in [Point::new(24.0, 32.0), Point::new(-3.25, 8.5)] {
            let mut child = Canvas::new(128, 128, scale);
            child.push_path(
                rect_path(1.0, 2.0, 9.0, 10.0),
                Color::WHITE,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
            );
            let mut parent = Canvas::new(128, 128, scale);
            parent.append(&child, offset);
            let local = parent.draw_records[0].pixel_bounds;
            assert_eq!(parent.draw_records[0].local_pixel_bounds, local);
            let transform = Affine::translate((16.0, 12.0)) * Affine::scale(1.5);
            parent.set_retained_transform(transform);
            assert_eq!(
                parent.draw_records[0].pixel_bounds,
                GpuAffine::from_logical(transform, scale).transform_bounds(local),
            );
        }
    }
}
