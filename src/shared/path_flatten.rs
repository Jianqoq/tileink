use peniko::kurbo::{BezPath, PathEl, Point};

use crate::{
    TILE_SIZE,
    shared::{cubic::CubicPoints, line::Line, vec2::Vec2},
};

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
                    if let (Some(start_pt), Some(end_pt)) = (contour_start, last) {
                        if is_open_contour_close(start_pt, end_pt) {
                            push_line_segment(
                                out,
                                self.tile_cnt,
                                self.tolerance,
                                self.path_id,
                                end_pt,
                                start_pt,
                            );
                        }
                    }
                    contour_start = Some(*point);
                    last = None;
                }
                PathEl::LineTo(point) => {
                    if let Some(p0) = last {
                        push_line_segment(
                            out,
                            self.tile_cnt,
                            self.tolerance,
                            self.path_id,
                            p0,
                            *point,
                        );
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
                        push_line_segment(
                            out,
                            self.tile_cnt,
                            self.tolerance,
                            self.path_id,
                            end_pt,
                            start_pt,
                        );
                        last = Some(start_pt);
                    }
                }
            }
        }
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
    tolerance: f32,
    path_id: u32,
    p0: Point,
    p1: Point,
) {
    if !is_non_degenerate_line(p0, p1) {
        return;
    }
    let cubic = Vec2::line_to_cubic(
        Vec2::new(p0.x as f32, p0.y as f32),
        Vec2::new(p1.x as f32, p1.y as f32),
    );
    CubicPoints::flatten_euler(cubic, cubic.p0, cubic.p3, tolerance, |p0, p1| {
        let line = Line {
            path_id,
            _pad: 0.0,
            p0: [p0.x, p0.y],
            p1: [p1.x, p1.y],
        };
        *tile_count = tile_count.saturating_add(tile_cover_upper_bound_for_line(&line));
        out.push(line);
    });
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
    let cubic = Vec2::quad_to_cubic(
        Vec2::new(p0.x as f32, p0.y as f32),
        Vec2::new(p1.x as f32, p1.y as f32),
        Vec2::new(p2.x as f32, p2.y as f32),
    );
    CubicPoints::flatten_euler(cubic, cubic.p0, cubic.p3, tolerance, |p0, p1| {
        let line = Line {
            path_id,
            _pad: 0.0,
            p0: [p0.x, p0.y],
            p1: [p1.x, p1.y],
        };
        *tile_count = tile_count.saturating_add(tile_cover_upper_bound_for_line(&line));
        out.push(line);
    });
}

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
    let cubic = CubicPoints {
        p0: Vec2::new(p0.x as f32, p0.y as f32),
        p1: Vec2::new(p1.x as f32, p1.y as f32),
        p2: Vec2::new(p2.x as f32, p2.y as f32),
        p3: Vec2::new(p3.x as f32, p3.y as f32),
    };
    CubicPoints::flatten_euler(cubic, cubic.p0, cubic.p3, tolerance, |p0, p1| {
        let line = Line {
            path_id,
            _pad: 0.0,
            p0: [p0.x, p0.y],
            p1: [p1.x, p1.y],
        };
        *tile_count = tile_count.saturating_add(tile_cover_upper_bound_for_line(&line));
        out.push(line);
    });
}

fn is_non_degenerate_line(p0: Point, p1: Point) -> bool {
    let dx = p0.x - p1.x;
    let dy = p0.y - p1.y;
    dx * dx + dy * dy > 1.0e-12 * 1.0e-12
}

fn tile_cover_upper_bound_for_line(line: &Line) -> u32 {
    let min_x = line.p0[0].min(line.p1[0]).floor().max(0.0) as u32 / TILE_SIZE;
    let min_y = line.p0[1].min(line.p1[1]).floor().max(0.0) as u32 / TILE_SIZE;
    let max_x = (line.p0[0].max(line.p1[0]).ceil().max(0.0) as u32).div_ceil(TILE_SIZE);
    let max_y = (line.p0[1].max(line.p1[1]).ceil().max(0.0) as u32).div_ceil(TILE_SIZE);

    max_x.saturating_sub(min_x) * max_y.saturating_sub(min_y)
}
