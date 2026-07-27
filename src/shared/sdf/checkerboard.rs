use peniko::kurbo::{Point, Rect};

use crate::shared::bounds::Bounds;

/// Analytic coverage for one parity of a rectangular checkerboard.
///
/// A complete two-color checkerboard uses one ordinary rectangle draw for the
/// base color and this SDF for the alternating cells. The draw count therefore
/// stays constant regardless of the covered area.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Checkerboard {
    pub start: Point,
    pub end: Point,
    pub cell_size: f32,
}

impl Checkerboard {
    pub fn new(rect: Rect, cell_size: f32) -> Self {
        let checkerboard = Self {
            start: Point::new(rect.x0, rect.y0),
            end: Point::new(rect.x1, rect.y1),
            cell_size,
        };
        assert!(
            !checkerboard.is_empty(),
            "SDF checkerboard rectangle and cell size must be finite and positive"
        );
        checkerboard
    }

    pub(crate) fn is_empty(self) -> bool {
        ![self.start.x, self.start.y, self.end.x, self.end.y]
            .into_iter()
            .all(f64::is_finite)
            || !self.cell_size.is_finite()
            || self.cell_size <= 0.0
            || self.start.x == self.end.x
            || self.start.y == self.end.y
    }

    pub(crate) fn bounds(self) -> Bounds {
        if self.is_empty() {
            return Bounds::new(0, 0, 0, 0);
        }
        let (x0, y0, x1, y1) = self.axis_bounds();
        Bounds::new(
            x0.floor() as i32,
            y0.floor() as i32,
            x1.ceil() as i32,
            y1.ceil() as i32,
        )
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.start.x -= f64::from(dx);
        self.start.y -= f64::from(dy);
        self.end.x -= f64::from(dx);
        self.end.y -= f64::from(dy);
        self
    }

    pub(crate) fn axis_bounds(self) -> (f64, f64, f64, f64) {
        (
            self.start.x.min(self.end.x),
            self.start.y.min(self.end.y),
            self.start.x.max(self.end.x),
            self.start.y.max(self.end.y),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_normalize_reversed_rectangles() {
        let checkerboard = Checkerboard::new(Rect::new(18.0, 19.0, 2.0, 3.0), 4.0);
        assert_eq!(checkerboard.bounds(), Bounds::new(2, 3, 18, 19));
    }

    #[test]
    fn invalid_public_literals_are_empty() {
        let invalid = Checkerboard {
            start: Point::ZERO,
            end: Point::new(16.0, 16.0),
            cell_size: f32::NAN,
        };
        assert!(invalid.is_empty());
    }
}
