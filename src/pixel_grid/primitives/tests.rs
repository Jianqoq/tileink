use super::*;
use peniko::kurbo::{BezPath, PathEl};

fn reference_path<const N: usize>(points: [Point; N], closed: bool) -> BezPath {
    let mut path = BezPath::new();
    if let Some(first) = points.first() {
        path.move_to(*first);
        for point in &points[1..] {
            path.line_to(*point);
        }
        if closed {
            path.close_path();
        }
    }
    path
}

fn vertices(path: &BezPath) -> Vec<Point> {
    path.elements()
        .iter()
        .filter_map(|element| match element {
            PathEl::MoveTo(point) | PathEl::LineTo(point) => Some(*point),
            PathEl::ClosePath => None,
            _ => panic!("primitive reference changed topology"),
        })
        .collect()
}

#[test]
fn typed_polygons_and_lines_match_path_fitting_at_fractional_dpi_zoom_and_reflections() {
    let triangle = [
        Point::new(2.2, 4.2),
        Point::new(16.2, 4.2),
        Point::new(2.2, 18.2),
    ];
    let rectangle = [
        triangle[0],
        triangle[1],
        Point::new(16.2, 18.2),
        triangle[2],
    ];
    for dpi in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let grid = PixelGrid::new(dpi).unwrap();
        for zoom in [0.8, 1.0, 1.25, 2.0] {
            for reflect in [-1.0, 1.0] {
                let transform = Affine::translate((20.3, 0.7))
                    * Affine::scale_non_uniform(zoom * reflect, zoom);
                assert_eq!(
                    grid.fill_polygon(triangle, transform).as_slice(),
                    vertices(&grid.fill_path(&reference_path(triangle, true), transform))
                );
                assert_eq!(
                    grid.fill_polygon(rectangle, transform).as_slice(),
                    vertices(&grid.fill_path(&reference_path(rectangle, true), transform))
                );
                for line in [
                    [triangle[0], triangle[1]],
                    [triangle[0], triangle[2]],
                    [triangle[1], triangle[2]],
                ] {
                    for width in [0.75, 1.0, 1.5, 2.0] {
                        let stroke = Stroke::new(width);
                        let (actual, actual_stroke) = grid.stroke_line(line, &stroke, transform);
                        let (path, fitted) =
                            grid.stroke_path(&reference_path(line, false), &stroke, transform);
                        assert_eq!(actual.as_slice(), vertices(&path));
                        assert_eq!(actual_stroke.width, fitted.width);
                        assert_eq!(grid.stroke_width(width, transform), fitted.width);
                    }
                }
            }
        }
    }
}

#[test]
fn typed_fitting_preserves_empty_degenerate_and_unsupported_geometry_contracts() {
    let grid = PixelGrid::new(2.0).unwrap();
    assert_eq!(grid.fill_polygon([], Affine::IDENTITY), []);
    let point = [Point::new(2.2, 4.2)];
    assert_eq!(
        grid.fill_polygon(point, Affine::IDENTITY).as_slice(),
        vertices(&grid.fill_path(&reference_path(point, true), Affine::IDENTITY))
    );
    let line = [Point::new(2.2, 4.2), Point::new(16.2, 4.2)];
    for transform in [
        Affine::rotate(0.3),
        Affine::skew(0.2, 0.0),
        Affine::scale_non_uniform(2.0, 1.0),
    ] {
        assert_eq!(grid.fill_polygon(line, transform), line);
        let (actual, stroke) = grid.stroke_line(line, &Stroke::new(1.25), transform);
        assert_eq!(actual, line);
        assert_eq!(stroke.width, 1.25);
    }
    for width in [0.0, -1.0, f64::INFINITY] {
        let (actual, stroke) = grid.stroke_line(line, &Stroke::new(width), Affine::IDENTITY);
        assert_eq!(actual, line);
        assert_eq!(stroke.width, width);
    }
    let extreme = PixelGrid::new(f64::MAX).unwrap();
    assert_eq!(extreme.fill_polygon(line, Affine::IDENTITY), line);
    let (actual, stroke) = extreme.stroke_line(line, &Stroke::new(2.0), Affine::IDENTITY);
    assert_eq!(actual, line);
    assert_eq!(stroke.width, 2.0);
}
