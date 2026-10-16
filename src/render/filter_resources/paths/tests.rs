use super::*;
use crate::Radius;
use crate::render::filter_resources::cursors::FilterCursors;
use peniko::kurbo::{Affine, BezPath, Rect};

#[test]
fn path_coordinates_keep_signed_half_step_rounding_and_saturation() {
    assert_eq!(encode_filter_path_coord(1.0 / 512.0), 1);
    assert_eq!(encode_filter_path_coord(-1.0 / 512.0), -1);
    assert_eq!(encode_filter_path_coord(0.25), 64);
    assert_eq!(encode_filter_path_coord(-0.25), -64);
    assert_eq!(encode_filter_path_coord(f32::MAX), i32::MAX);
    assert_eq!(encode_filter_path_coord(-f32::MAX), i32::MIN);
}

#[test]
fn empty_paths_reserve_indices_and_rectangles_do_not() {
    let rect = Region::Rect {
        rect: Rect::new(0.0, 0.0, 17.0, 19.0),
        radius: Radius::ZERO,
    };
    let empty = Region::Path {
        path: BezPath::new(),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
    };
    let mut path = BezPath::new();
    path.move_to((0.5, 0.25));
    path.line_to((2.5, 3.25));
    let line = Region::Path {
        path,
        transform: Affine::translate((1.0, 2.0)),
        tolerance: 0.1,
    };
    let mut upload = FilterPathUpload::default();
    let mut cursor = FilterCursors::default();
    for (region, expected) in [(&rect, None), (&empty, Some(0)), (&line, Some(1))] {
        upload.push_region(region);
        assert_eq!(cursor.next_path_index(region), expected);
    }
    assert_eq!(upload.range_starts, [0, 0]);
    assert_eq!(upload.range_ends[0], 0);
    assert_eq!(upload.range_ends[1] as usize, upload.p0x.len());
    assert!(!upload.p0x.is_empty());
    assert!((0..upload.p0x.len()).any(|i| [
        upload.p0x[i],
        upload.p0y[i],
        upload.p1x[i],
        upload.p1y[i]
    ] == [384, 576, 896, 1344]));
}
