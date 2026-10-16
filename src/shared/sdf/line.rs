use peniko::kurbo::Point;

use super::shadow::{ShadowOptions, shadow_bounds};
use crate::shared::bounds::Bounds;

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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DashLine {
    pub line: Line,
    pub dash_length: f32,
    pub gap_length: f32,
    pub dash_offset: f32,
}

impl Line {
    #[must_use]
    ///
    /// # Panics
    /// Panics if stroke width is nonpositive or NaN.
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
        // A point has no butt-capped area. Every positive segment retains its transformed AA
        // geometry; an absolute logical-pixel epsilon erased magnified short strokes.
        self.width <= 0.0 || (self.cap == LineCap::Butt && self.start == self.end)
    }

    pub(crate) fn bounds(self) -> Bounds {
        if self.is_empty() {
            return Bounds::new(0, 0, 0, 0);
        }

        let half = f64::from(self.width) * 0.5;
        if let Some((ux, uy)) = self.unit_axis() {
            let (mut x0, mut y0) = (self.start.x, self.start.y);
            let (mut x1, mut y1) = (self.end.x, self.end.y);
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
                (self.start.x - half).floor() as i32,
                (self.start.y - half).floor() as i32,
                (self.start.x + half).ceil() as i32,
                (self.start.y + half).ceil() as i32,
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

    pub(crate) const fn cap_value(self) -> f32 {
        self.cap as u32 as f32
    }

    fn unit_axis(self) -> Option<(f64, f64)> {
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        let len = dx.hypot(dy);
        if len == 0.0 {
            None
        } else {
            Some((dx / len, dy / len))
        }
    }
}

impl DashLine {
    #[must_use]
    ///
    /// # Panics
    /// Panics if dash length or stroke width is nonpositive/NaN, or gap length is negative/NaN.
    pub fn new(
        start: Point,
        end: Point,
        width: f32,
        cap: LineCap,
        dash_length: f32,
        gap_length: f32,
    ) -> Self {
        Self::with_offset(start, end, width, cap, dash_length, gap_length, 0.0)
    }

    #[must_use]
    ///
    /// # Panics
    /// Panics if dash length is nonpositive/NaN or gap length is negative/NaN. Stroke width must also be positive.
    pub fn with_offset(
        start: Point,
        end: Point,
        width: f32,
        cap: LineCap,
        dash_length: f32,
        gap_length: f32,
        dash_offset: f32,
    ) -> Self {
        assert!(dash_length > 0.0, "SDF dash length must be positive");
        assert!(gap_length >= 0.0, "SDF dash gap must be non-negative");
        Self {
            line: Line::new(start, end, width, cap),
            dash_length,
            gap_length,
            dash_offset,
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        if self.line.is_empty() || self.dash_length <= 0.0 {
            return true;
        }
        let Some(len) = self.line_len() else {
            return false;
        };
        self.gap_length > 0.0 && !self.has_visible_dash(len)
    }

    pub(crate) fn bounds(self) -> Bounds {
        if self.is_empty() {
            Bounds::new(0, 0, 0, 0)
        } else {
            self.line.bounds()
        }
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.line = self.line.translated(dx, dy);
        self
    }

    fn line_len(self) -> Option<f64> {
        let dx = self.line.end.x - self.line.start.x;
        let dy = self.line.end.y - self.line.start.y;
        let len = dx.hypot(dy);
        (len > 0.0).then_some(len)
    }

    fn has_visible_dash(self, len: f64) -> bool {
        // Test the current dash or the next start directly. Widening keeps finite f32 dash/gap
        // sums finite, and an exact positive gap must not turn into a continuous short line.
        let dash = f64::from(self.dash_length);
        let cycle = dash + f64::from(self.gap_length);
        let offset = f64::from(self.dash_offset).rem_euclid(cycle);
        offset < dash || cycle - offset < len
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LineShadow {
    pub line: Line,
    pub options: ShadowOptions,
}

impl LineShadow {
    pub(crate) fn bounds(self) -> Bounds {
        shadow_bounds(self.line.bounds(), self.options)
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.line = self.line.translated(dx, dy);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_lines_and_dash_gaps_keep_their_authored_geometry() {
        for length in [1.0e-20, 1.0e-10, 1.0e-6, 1.0, 1.0e20] {
            let line = Line::new(Point::ZERO, Point::new(length, 0.0), 1.0, LineCap::Butt);
            assert!(!line.is_empty(), "length {length}");
            assert!(line.bounds().width() > 0);
            let dash = DashLine::with_offset(
                Point::ZERO,
                Point::new(length * 0.25, 0.0),
                1.0,
                LineCap::Butt,
                length as f32,
                length as f32,
                length as f32,
            );
            assert!(
                dash.is_empty(),
                "a short line entirely within a nonzero gap is empty"
            );
        }
    }

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
