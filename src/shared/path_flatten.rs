use peniko::kurbo::{BezPath, PathEl, Point};

use crate::{TILE_SIZE, shared::line::Line};

const MAX_FLATTEN_DEPTH: u32 = 20;

pub struct PathFlatten<'a> {
    path: &'a BezPath,
    tolerance: f32,
    path_id: u32,
    tile_cnt: &'a mut u32,
}

impl<'a> PathFlatten<'a> {
    pub fn new(path: &'a BezPath, tolerance: f32, path_id: u32, tile_cnt: &'a mut u32) -> Self {
        Self {
            path,
            tolerance,
            path_id,
            tile_cnt,
        }
    }

    pub fn flatten(&mut self, out: &mut Vec<Line>) {
        let mut contour_start: Option<Point> = None;
        let mut last: Option<Point> = None;
        for el in self.path.elements() {
            match el {
                PathEl::MoveTo(point) => {
                    if let (Some(start_pt), Some(end_pt)) = (contour_start, last)
                        && is_open_contour_close(start_pt, end_pt)
                    {
                        push_line_segment(out, self.tile_cnt, self.path_id, end_pt, start_pt);
                    }
                    contour_start = Some(*point);
                    last = Some(*point);
                }
                PathEl::LineTo(point) => {
                    if let Some(p0) = last {
                        push_line_segment(out, self.tile_cnt, self.path_id, p0, *point);
                    }
                    last = Some(*point);
                }
                PathEl::QuadTo(p1, p2) => {
                    if let Some(p0) = last {
                        push_quad_segment(
                            out,
                            self.tile_cnt,
                            self.tolerance,
                            self.path_id,
                            p0,
                            *p1,
                            *p2,
                        );
                    }
                    last = Some(*p2);
                }
                PathEl::CurveTo(p1, p2, p3) => {
                    if let Some(p0) = last {
                        push_cubic_segment(
                            out,
                            self.tile_cnt,
                            self.tolerance,
                            self.path_id,
                            p0,
                            *p1,
                            *p2,
                            *p3,
                        );
                    }
                    last = Some(*p3);
                }
                PathEl::ClosePath => {
                    if let (Some(end_pt), Some(start_pt)) = (last, contour_start) {
                        push_line_segment(out, self.tile_cnt, self.path_id, end_pt, start_pt);
                        last = Some(start_pt);
                    }
                }
            }
        }
        if let (Some(start_pt), Some(end_pt)) = (contour_start, last)
            && is_open_contour_close(start_pt, end_pt)
        {
            push_line_segment(out, self.tile_cnt, self.path_id, end_pt, start_pt);
        }
    }
}

#[cfg(test)]
mod tests {
    use peniko::kurbo::{BezPath, Circle, PathEl, Shape};

    use super::PathFlatten;

    #[test]
    fn line_after_move_to_is_not_skipped() {
        let path = BezPath::from_vec(vec![
            PathEl::MoveTo((1.0, 2.0).into()),
            PathEl::LineTo((5.0, 2.0).into()),
        ]);
        let mut tile_cnt = 0;
        let mut lines = Vec::new();

        PathFlatten::new(&path, 0.1, 0, &mut tile_cnt).flatten(&mut lines);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].p0, [1.0, 2.0]);
        assert_eq!(lines[0].p1, [5.0, 2.0]);
        assert_eq!(lines[1].p0, [5.0, 2.0]);
        assert_eq!(lines[1].p1, [1.0, 2.0]);
    }

    #[test]
    fn final_open_contour_is_closed() {
        let path = BezPath::from_vec(vec![
            PathEl::MoveTo((0.0, 0.0).into()),
            PathEl::LineTo((10.0, 0.0).into()),
            PathEl::LineTo((10.0, 10.0).into()),
        ]);
        let mut tile_cnt = 0;
        let mut lines = Vec::new();

        PathFlatten::new(&path, 0.1, 0, &mut tile_cnt).flatten(&mut lines);

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2].p0, [10.0, 10.0]);
        assert_eq!(lines[2].p1, [0.0, 0.0]);
    }

    #[test]
    fn closed_rect_keeps_all_edges() {
        let path = BezPath::from_vec(vec![
            PathEl::MoveTo((0.0, 0.0).into()),
            PathEl::LineTo((10.0, 0.0).into()),
            PathEl::LineTo((10.0, 10.0).into()),
            PathEl::LineTo((0.0, 10.0).into()),
            PathEl::ClosePath,
        ]);
        let mut tile_cnt = 0;
        let mut lines = Vec::new();

        PathFlatten::new(&path, 0.1, 0, &mut tile_cnt).flatten(&mut lines);

        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn open_horizontal_line_closes_and_reserves_segment_capacity() {
        let path = BezPath::from_vec(vec![
            PathEl::MoveTo((0.0, 0.0).into()),
            PathEl::LineTo((100.0, 0.0).into()),
        ]);
        let mut tile_cnt = 0;
        let mut lines = Vec::new();

        PathFlatten::new(&path, 0.1, 0, &mut tile_cnt).flatten(&mut lines);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].p0, [100.0, 0.0]);
        assert_eq!(lines[1].p1, [0.0, 0.0]);
        assert!(tile_cnt >= 7);
    }

    #[test]
    fn circle_flattens_to_many_small_lines() {
        let path = Circle::new((20.0, 20.0), 10.0).to_path(0.1);
        let mut tile_cnt = 0;
        let mut lines = Vec::new();

        PathFlatten::new(&path, 0.1, 0, &mut tile_cnt).flatten(&mut lines);

        assert!(lines.len() > 16);
        assert!(lines.iter().all(|line| line.p0[0] >= 9.0
            && line.p0[0] <= 31.0
            && line.p0[1] >= 9.0
            && line.p0[1] <= 31.0
            && line.p1[0] >= 9.0
            && line.p1[0] <= 31.0
            && line.p1[1] >= 9.0
            && line.p1[1] <= 31.0));
    }
}

fn is_open_contour_close(start: Point, end: Point) -> bool {
    let dx = start.x - end.x;
    let dy = start.y - end.y;
    dx * dx + dy * dy > 1.0e-6 * 1.0e-6
}

fn push_line_segment(
    out: &mut Vec<Line>,
    tile_count: &mut u32,
    path_id: u32,
    p0: Point,
    p1: Point,
) {
    if !is_non_degenerate_line(p0, p1) {
        return;
    }
    push_flat_line(out, tile_count, path_id, p0, p1);
}

fn push_quad_segment(
    out: &mut Vec<Line>,
    tile_count: &mut u32,
    tolerance: f32,
    path_id: u32,
    p0: Point,
    p1: Point,
    p2: Point,
) {
    flatten_quad(out, tile_count, tolerance as f64, path_id, p0, p1, p2, 0);
}

#[allow(clippy::too_many_arguments)]
fn push_cubic_segment(
    out: &mut Vec<Line>,
    tile_count: &mut u32,
    tolerance: f32,
    path_id: u32,
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
) {
    flatten_cubic(
        out,
        tile_count,
        tolerance as f64,
        path_id,
        p0,
        p1,
        p2,
        p3,
        0,
    );
}

fn push_flat_line(out: &mut Vec<Line>, tile_count: &mut u32, path_id: u32, p0: Point, p1: Point) {
    if !is_non_degenerate_line(p0, p1) {
        return;
    }
    let line = Line {
        path_id,
        _pad: 0.0,
        p0: [p0.x as f32, p0.y as f32],
        p1: [p1.x as f32, p1.y as f32],
    };
    *tile_count = tile_count.saturating_add(tile_cover_upper_bound_for_line(&line));
    out.push(line);
}

#[allow(clippy::too_many_arguments)]
fn flatten_quad(
    out: &mut Vec<Line>,
    tile_count: &mut u32,
    tolerance: f64,
    path_id: u32,
    p0: Point,
    p1: Point,
    p2: Point,
    depth: u32,
) {
    if depth >= MAX_FLATTEN_DEPTH || point_line_distance(p1, p0, p2) <= tolerance {
        push_flat_line(out, tile_count, path_id, p0, p2);
        return;
    }

    let p01 = midpoint(p0, p1);
    let p12 = midpoint(p1, p2);
    let p012 = midpoint(p01, p12);

    flatten_quad(
        out,
        tile_count,
        tolerance,
        path_id,
        p0,
        p01,
        p012,
        depth + 1,
    );
    flatten_quad(
        out,
        tile_count,
        tolerance,
        path_id,
        p012,
        p12,
        p2,
        depth + 1,
    );
}

#[allow(clippy::too_many_arguments)]
fn flatten_cubic(
    out: &mut Vec<Line>,
    tile_count: &mut u32,
    tolerance: f64,
    path_id: u32,
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    depth: u32,
) {
    let flatness = point_line_distance(p1, p0, p3).max(point_line_distance(p2, p0, p3));
    if depth >= MAX_FLATTEN_DEPTH || flatness <= tolerance {
        push_flat_line(out, tile_count, path_id, p0, p3);
        return;
    }

    let p01 = midpoint(p0, p1);
    let p12 = midpoint(p1, p2);
    let p23 = midpoint(p2, p3);
    let p012 = midpoint(p01, p12);
    let p123 = midpoint(p12, p23);
    let p0123 = midpoint(p012, p123);

    flatten_cubic(
        out,
        tile_count,
        tolerance,
        path_id,
        p0,
        p01,
        p012,
        p0123,
        depth + 1,
    );
    flatten_cubic(
        out,
        tile_count,
        tolerance,
        path_id,
        p0123,
        p123,
        p23,
        p3,
        depth + 1,
    );
}

fn midpoint(a: Point, b: Point) -> Point {
    Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

fn point_line_distance(p: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len = dx.hypot(dy);
    if len <= 1.0e-12 {
        return (p.x - a.x).hypot(p.y - a.y);
    }
    ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() / len
}

fn is_non_degenerate_line(p0: Point, p1: Point) -> bool {
    let dx = p0.x - p1.x;
    let dy = p0.y - p1.y;
    dx * dx + dy * dy > 1.0e-12 * 1.0e-12
}

fn tile_cover_upper_bound_for_line(line: &Line) -> u32 {
    let p0 = (line.p0[0] / TILE_SIZE as f32, line.p0[1] / TILE_SIZE as f32);
    let p1 = (line.p1[0] / TILE_SIZE as f32, line.p1[1] / TILE_SIZE as f32);

    span(p0.0, p1.0) + span(p0.1, p1.1) + 8
}

fn span(a: f32, b: f32) -> u32 {
    let hi = a.max(b).ceil();
    let lo = a.min(b).floor();
    (hi - lo).max(1.0) as u32
}
