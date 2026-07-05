use super::*;

fn assert_path_geometry_records_match_scene(canvas: &Canvas) {
    assert!(!canvas.lines.is_empty());
    assert_eq!(canvas.path_records.len(), canvas.path_cnt as usize);
    for record in &canvas.path_records {
        assert!(record.path_id < canvas.path_cnt);
        let line_end = record.line_start + record.line_count;
        assert!(line_end as usize <= canvas.lines.len());
    }
}

#[test]
fn push_arc_adds_draw_and_path_record() {
    let mut canvas = test_scene();
    canvas.push_arc(
        Arc::new((16.0, 16.0), (8.0, 6.0), 0.0, std::f64::consts::PI, 0.0),
        Brush::Solid(rgb(0, 255, 0)),
        FillRule::NonZero,
        0.25,
    );

    assert_eq!(canvas.draw_records.len(), 1);
    assert_eq!(canvas.path_records.len(), 1);
    assert_eq!(canvas.path_records.len(), 1);
    assert_eq!(canvas.draw_records[0].tag, DrawTag::Brush);
    assert!(!canvas.draw_records[0].solid_rect());
}

#[test]
fn push_stroke_expands_shape_to_fill_path() {
    let mut canvas = test_scene();
    canvas.push_stroke(
        Rect::new(10.0, 10.0, 20.0, 20.0),
        Stroke::new(4.0),
        Brush::Solid(rgb(255, 0, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    assert_eq!(canvas.draw_records.len(), 1);
    let bounds = canvas.draw_records[0].pixel_bounds;
    assert!(bounds.x0 <= 8);
    assert!(bounds.y0 <= 8);
    assert!(bounds.x1 >= 22);
    assert!(bounds.y1 >= 22);
    assert_eq!(canvas.draw_records[0].fill_rule, FillRule::NonZero);
    assert!(!canvas.draw_records[0].solid_rect());
}

#[test]
fn push_path_flattens_transformed_geometry() {
    let mut canvas = test_scene();
    canvas.push_path(
        rect_path(0.0, 0.0, 10.0, 10.0),
        Brush::Solid(rgb(255, 0, 0)),
        Affine::translate((8.0, 4.0)),
        FillRule::NonZero,
        0.25,
    );

    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 8,
            y0: 4,
            x1: 18,
            y1: 14,
        }
    );
    assert!(canvas.lines.iter().all(|line| {
        [line.p0, line.p1]
            .into_iter()
            .all(|point| point[0] >= 8.0 && point[0] <= 18.0 && point[1] >= 4.0 && point[1] <= 14.0)
    }));
    assert_path_geometry_records_match_scene(&canvas);
}

#[test]
fn push_path_reserves_segment_capacity_from_scan_tile_count() {
    let mut path = BezPath::new();
    path.move_to((8.0, 8.0));
    path.line_to((9.0, 12.0));

    let mut canvas = test_scene();
    canvas.push_path(
        path,
        Brush::Solid(rgb(255, 0, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );

    let record = canvas.path_records[0];
    let tile_bbox = crate::shared::bounds::TileBbox {
        x0: record.tile_x0,
        y0: record.tile_y0,
        x1: record.tile_x1,
        y1: record.tile_y1,
    };
    let expected = canvas
        .lines
        .iter()
        .map(|&line| {
            line_scanned_tile_count(
                line,
                tile_bbox,
                (canvas.width_in_tiles(), canvas.height_in_tiles()),
            )
        })
        .sum::<u32>();

    assert_eq!(record.segment_capacity, expected);
    assert_eq!(canvas.tile_cnt, expected);
    assert!(expected > 0);
    assert!(expected < 20);
}

#[test]
fn push_layer_path_flattens_transformed_geometry() {
    let mut canvas = test_scene();
    canvas.push_clip_layer(
        rect_path(0.0, 0.0, 10.0, 10.0),
        Affine::translate((12.0, 6.0)),
        FillRule::NonZero,
        0.25,
    );

    assert_eq!(
        canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 12,
            y0: 6,
            x1: 22,
            y1: 16,
        }
    );
    assert!(canvas.lines.iter().all(|line| {
        [line.p0, line.p1].into_iter().all(|point| {
            point[0] >= 12.0 && point[0] <= 22.0 && point[1] >= 6.0 && point[1] <= 16.0
        })
    }));
    assert_path_geometry_records_match_scene(&canvas);
}

#[test]
fn append_preserves_path_geometry_for_scene_records() {
    let mut child = test_scene();
    child.push_path(
        rect_path(0.0, 0.0, 10.0, 10.0),
        Brush::Solid(rgb(255, 0, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );

    let mut canvas = test_scene();
    canvas.append(&child, Point::new(8.0, 4.0));

    assert_path_geometry_records_match_scene(&canvas);
    assert!(canvas.lines.iter().all(|line| {
        [line.p0, line.p1]
            .into_iter()
            .all(|point| point[0] >= 8.0 && point[0] <= 18.0 && point[1] >= 4.0 && point[1] <= 14.0)
    }));
}

#[test]
fn append_preserves_path_geometry_for_scene_records_without_mutating_child() {
    let mut child = test_scene();
    child.push_path(
        rect_path(0.0, 0.0, 10.0, 10.0),
        Brush::Solid(rgb(255, 0, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );
    let original_lines = child.lines.clone();
    let original_paths = child.path_records.clone();

    let mut canvas = test_scene();
    canvas.append(&child, Point::new(8.0, 4.0));
    canvas.append(&child, Point::new(20.0, 12.0));

    assert_eq!(child.lines.len(), original_lines.len());
    for (actual, expected) in child.lines.iter().zip(&original_lines) {
        assert_eq!(actual.path_id, expected.path_id);
        assert_eq!(actual.p0, expected.p0);
        assert_eq!(actual.p1, expected.p1);
    }
    assert_eq!(child.path_records.len(), original_paths.len());
    for (actual, expected) in child.path_records.iter().zip(&original_paths) {
        assert_eq!(actual.path_id, expected.path_id);
        assert_eq!(actual.line_start, expected.line_start);
        assert_eq!(actual.line_count, expected.line_count);
        assert_eq!(actual.flags, expected.flags);
    }
    assert_path_geometry_records_match_scene(&canvas);
    assert_eq!(canvas.path_records.len(), child.path_records.len() * 2);
    assert_eq!(canvas.path_records.len(), canvas.path_records.len());
    assert!(canvas.lines.iter().all(|line| {
        [line.p0, line.p1].into_iter().all(|point| {
            (point[0] >= 8.0 && point[0] <= 30.0) && (point[1] >= 4.0 && point[1] <= 22.0)
        })
    }));
}
