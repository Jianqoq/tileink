use super::*;

#[test]
fn push_arc_adds_draw_and_path_record() {
    let mut scene = test_scene();
    scene.push_arc(
        Arc::new((16.0, 16.0), (8.0, 6.0), 0.0, std::f64::consts::PI, 0.0),
        Brush::Solid(rgb(0, 255, 0)),
        FillRule::NonZero,
        0.25,
    );

    assert_eq!(scene.draw_records.len(), 1);
    assert_eq!(scene.path_records.len(), 1);
    assert_eq!(scene.bd_records.len(), 1);
    assert_eq!(scene.draw_records[0].tag, DrawTag::Brush);
    assert!(!scene.draw_records[0].solid_rect);
}

#[test]
fn push_stroke_expands_shape_to_fill_path() {
    let mut scene = test_scene();
    scene.push_stroke(
        Rect::new(10.0, 10.0, 20.0, 20.0),
        Stroke::new(4.0),
        Brush::Solid(rgb(255, 0, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    assert_eq!(scene.draw_records.len(), 1);
    let bounds = scene.draw_records[0].pixel_bounds;
    assert!(bounds.x0 <= 8);
    assert!(bounds.y0 <= 8);
    assert!(bounds.x1 >= 22);
    assert!(bounds.y1 >= 22);
    assert_eq!(scene.draw_records[0].fill_rule, FillRule::NonZero);
    assert!(!scene.draw_records[0].solid_rect);
}

#[test]
fn push_path_flattens_transformed_geometry() {
    let mut scene = test_scene();
    scene.push_path(
        rect_path(0.0, 0.0, 10.0, 10.0),
        Brush::Solid(rgb(255, 0, 0)),
        Affine::translate((8.0, 4.0)),
        FillRule::NonZero,
        0.25,
    );

    assert_eq!(
        scene.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 8,
            y0: 4,
            x1: 18,
            y1: 14,
        }
    );
    assert!(scene.lines.iter().all(|line| {
        [line.p0, line.p1]
            .into_iter()
            .all(|point| point[0] >= 8.0 && point[0] <= 18.0 && point[1] >= 4.0 && point[1] <= 14.0)
    }));
}

#[test]
fn push_path_reserves_segment_capacity_from_scan_tile_count() {
    let mut path = BezPath::new();
    path.move_to((8.0, 8.0));
    path.line_to((9.0, 12.0));

    let mut scene = test_scene();
    scene.push_path(
        path,
        Brush::Solid(rgb(255, 0, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );

    let record = scene.bd_records[0];
    let tile_bbox = crate::shared::bounds::TileBbox {
        x0: record.tile_x0,
        y0: record.tile_y0,
        x1: record.tile_x1,
        y1: record.tile_y1,
    };
    let expected = scene
        .lines
        .iter()
        .map(|&line| {
            line_scanned_tile_count(
                line,
                tile_bbox,
                (scene.width_in_tiles(), scene.height_in_tiles()),
            )
        })
        .sum::<u32>();

    assert_eq!(record.segment_capacity, expected);
    assert_eq!(scene.tile_cnt, expected);
    assert!(expected > 0);
    assert!(expected < 20);
}

#[test]
fn push_layer_path_flattens_transformed_geometry() {
    let mut scene = test_scene();
    scene.push_clip_layer(
        rect_path(0.0, 0.0, 10.0, 10.0),
        Affine::translate((12.0, 6.0)),
        FillRule::NonZero,
        0.25,
    );

    assert_eq!(
        scene.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 12,
            y0: 6,
            x1: 22,
            y1: 16,
        }
    );
    assert!(scene.lines.iter().all(|line| {
        [line.p0, line.p1].into_iter().all(|point| {
            point[0] >= 12.0 && point[0] <= 22.0 && point[1] >= 6.0 && point[1] <= 16.0
        })
    }));
}
