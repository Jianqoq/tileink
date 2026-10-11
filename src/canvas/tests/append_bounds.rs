use super::*;

#[test]
fn path_only_backdrop_allocation_uses_geometry_and_preserves_empty_paths() {
    for (path, expected) in [
        (
            BezPath::new(),
            PixelBounds {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            },
        ),
        (
            rect_path(-8.5, 2.25, 36.5, 25.25),
            PixelBounds {
                x0: -9,
                y0: 2,
                x1: 37,
                y1: 26,
            },
        ),
        (
            rect_path(1.5, -4.5, 17.25, 35.75),
            PixelBounds {
                x0: 1,
                y0: -5,
                x1: 18,
                y1: 36,
            },
        ),
    ] {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_clip_layer(path, Affine::IDENTITY, FillRule::NonZero, 0.1);
        canvas.pop_layer();
        let previous_backdrops = canvas.backdrop_pool_capacity;
        // Layer geometry can be allocated without an ordinary draw in this range.
        canvas.append_path_backdrops_for_paths(0, canvas.path_records.len(), 0, 0);
        let record = canvas.path_records[0];
        let bbox = expected.tile_bbox(canvas.width_in_tiles(), canvas.height_in_tiles());
        assert_eq!(
            (
                record.tile_x0,
                record.tile_y0,
                record.tile_x1,
                record.tile_y1
            ),
            (bbox.x0, bbox.y0, bbox.x1, bbox.y1)
        );
        assert_eq!(record.data_offset, previous_backdrops);
        assert_eq!(record.data_len, bbox.tile_count());
        if expected.x0 == expected.x1 {
            assert_eq!(record.segment_capacity, 0);
        } else {
            assert!(record.segment_capacity > 0);
        }
    }
}

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
