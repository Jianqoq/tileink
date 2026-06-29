use peniko::kurbo::{BezPath, PathEl, Point, flatten};

use crate::{TILE_SIZE, shared::line::Line};

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
        // Kurbo's flatten uses a curve-aware subdivision estimate, so it keeps
        // the same tolerance contract without over-splitting long SVG curves.
        flatten(self.path.iter(), self.tolerance as f64, |el| match el {
            PathEl::MoveTo(point) => {
                if let (Some(start_pt), Some(end_pt)) = (contour_start, last)
                    && is_open_contour_close(start_pt, end_pt)
                {
                    push_line_segment(out, self.tile_cnt, self.path_id, end_pt, start_pt);
                }
                contour_start = Some(point);
                last = Some(point);
            }
            PathEl::LineTo(point) => {
                if let Some(p0) = last {
                    push_line_segment(out, self.tile_cnt, self.path_id, p0, point);
                }
                last = Some(point);
            }
            PathEl::ClosePath => {
                if let (Some(end_pt), Some(start_pt)) = (last, contour_start) {
                    push_line_segment(out, self.tile_cnt, self.path_id, end_pt, start_pt);
                    last = Some(start_pt);
                }
            }
            PathEl::QuadTo(..) | PathEl::CurveTo(..) => {
                unreachable!("kurbo::flatten only emits move, line, and close elements")
            }
        });
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
