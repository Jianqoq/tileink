use peniko::kurbo::Point;

use super::coverage_from_dist;
use crate::{TILE_SIZE, shared::bounds::Bounds};

const LINE_EPSILON: f32 = 1.0e-6;

#[repr(u32)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LineCap {
    Butt = 0,
    Square = 1,
    Round = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub start: Point,
    pub end: Point,
    pub width: f32,
    pub cap: LineCap,
}

impl Line {
    pub fn new(start: Point, end: Point, width: f32, cap: LineCap) -> Self {
        assert!(width > 0.0, "SDF line width must be positive");
        Self {
            start,
            end,
            width,
            cap,
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.width <= 0.0
            || (self.cap == LineCap::Butt
                && (self.end.x as f32 - self.start.x as f32).abs() <= LINE_EPSILON
                && (self.end.y as f32 - self.start.y as f32).abs() <= LINE_EPSILON)
    }

    pub(crate) fn bounds(self) -> Bounds {
        if self.is_empty() {
            return Bounds::new(0, 0, 0, 0);
        }

        let half = self.width * 0.5;
        if let Some((ux, uy)) = self.unit_axis() {
            let (mut x0, mut y0) = (self.start.x as f32, self.start.y as f32);
            let (mut x1, mut y1) = (self.end.x as f32, self.end.y as f32);
            if self.cap == LineCap::Square {
                x0 -= ux * half;
                y0 -= uy * half;
                x1 += ux * half;
                y1 += uy * half;
            }
            Bounds::new(
                (x0.min(x1) - half).floor() as i32,
                (y0.min(y1) - half).floor() as i32,
                (x0.max(x1) + half).ceil() as i32,
                (y0.max(y1) + half).ceil() as i32,
            )
        } else {
            Bounds::new(
                (self.start.x as f32 - half).floor() as i32,
                (self.start.y as f32 - half).floor() as i32,
                (self.start.x as f32 + half).ceil() as i32,
                (self.start.y as f32 + half).ceil() as i32,
            )
        }
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.start.x -= f64::from(dx);
        self.start.y -= f64::from(dy);
        self.end.x -= f64::from(dx);
        self.end.y -= f64::from(dy);
        self
    }

    pub(crate) fn tile_is_solid(self, _: Bounds) -> bool {
        false
    }

    pub(crate) fn fine_area(
        self,
        area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize],
        tile_bounds: Bounds,
        pixel_bounds: Bounds,
    ) {
        if self.is_empty() {
            return;
        }

        for y_px in pixel_bounds.y0..pixel_bounds.y1 {
            let py = y_px as f32 + 0.5;
            let row = (y_px - tile_bounds.y0) as usize * TILE_SIZE as usize;
            for x_px in pixel_bounds.x0..pixel_bounds.x1 {
                let px = x_px as f32 + 0.5;
                let ix = row + (x_px - tile_bounds.x0) as usize;
                area[ix] = coverage_from_dist(self.signed_distance(px, py));
            }
        }
    }

    pub(crate) fn cap_value(self) -> f32 {
        self.cap as u32 as f32
    }

    fn unit_axis(self) -> Option<(f32, f32)> {
        let dx = self.end.x as f32 - self.start.x as f32;
        let dy = self.end.y as f32 - self.start.y as f32;
        let len = dx.hypot(dy);
        if len <= LINE_EPSILON {
            None
        } else {
            Some((dx / len, dy / len))
        }
    }

    fn signed_distance(self, x: f32, y: f32) -> f32 {
        let half = self.width * 0.5;
        let sx = self.start.x as f32;
        let sy = self.start.y as f32;
        let ex = self.end.x as f32;
        let ey = self.end.y as f32;
        let dx = ex - sx;
        let dy = ey - sy;
        let len = dx.hypot(dy);
        if len <= LINE_EPSILON {
            return match self.cap {
                LineCap::Butt => f32::INFINITY,
                LineCap::Round => (x - sx).hypot(y - sy) - half,
                LineCap::Square => local_rect_distance(0.0, 0.0, -half, half, half),
            };
        }

        let ux = dx / len;
        let uy = dy / len;
        let px = x - sx;
        let py = y - sy;
        let axis = px * ux + py * uy;
        let normal = -px * uy + py * ux;
        match self.cap {
            LineCap::Butt => local_rect_distance(axis, normal, 0.0, len, half),
            LineCap::Square => local_rect_distance(axis, normal, -half, len + half, half),
            LineCap::Round => {
                let nearest = axis.clamp(0.0, len);
                (axis - nearest).hypot(normal) - half
            }
        }
    }
}

fn local_rect_distance(axis: f32, normal: f32, x0: f32, x1: f32, half_height: f32) -> f32 {
    let center = (x0 + x1) * 0.5;
    let half_width = (x1 - x0) * 0.5;
    let dx = (axis - center).abs() - half_width;
    let dy = normal.abs() - half_height;
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn butt_bounds_include_stroke_width_without_cap_extension() {
        let line = Line::new(
            Point::new(8.0, 16.5),
            Point::new(24.0, 16.5),
            1.0,
            LineCap::Butt,
        );

        assert_eq!(line.bounds(), Bounds::new(7, 16, 25, 17));
    }

    #[test]
    fn square_bounds_extend_by_half_width_along_axis() {
        let line = Line::new(
            Point::new(8.0, 16.5),
            Point::new(24.0, 16.5),
            2.0,
            LineCap::Square,
        );

        assert_eq!(line.bounds(), Bounds::new(6, 15, 26, 18));
    }

    #[test]
    fn zero_length_butt_line_is_empty() {
        let line = Line::new(
            Point::new(8.0, 8.0),
            Point::new(8.0, 8.0),
            4.0,
            LineCap::Butt,
        );

        assert!(line.is_empty());
        assert_eq!(line.bounds(), Bounds::new(0, 0, 0, 0));
    }
}
