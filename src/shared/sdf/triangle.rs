use peniko::kurbo::Point;

use crate::shared::bounds::Bounds;

/// Filled triangle with a uniform rounded-corner radius.
///
/// `corner_radius` is a Minkowski expansion of the three line segments. A zero radius preserves
/// the exact triangle; positive values round every vertex and expand the outer bounds uniformly.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub a: Point,
    pub b: Point,
    pub c: Point,
    pub corner_radius: f32,
}

impl Triangle {
    #[must_use]
    ///
    /// # Panics
    /// Panics for nonfinite/collinear points, an area outside the finite `f64` range,
    /// or a nonfinite/negative corner radius.
    pub fn new(a: Point, b: Point, c: Point, corner_radius: f32) -> Self {
        let triangle = Self {
            a,
            b,
            c,
            corner_radius,
        };
        assert!(
            !triangle.is_empty(),
            "SDF triangle points and area must be finite and non-collinear, and corner radius must be finite and non-negative"
        );
        triangle
    }

    pub(crate) fn is_empty(self) -> bool {
        let finite = [self.a.x, self.a.y, self.b.x, self.b.y, self.c.x, self.c.y]
            .into_iter()
            .all(f64::is_finite);
        let area = self.signed_double_area();
        // Positive subpixel triangles remain geometry. An absolute area threshold made valid
        // tiny gfx_ui elements panic before either hit testing or GPU encoding could run.
        !finite
            || !self.corner_radius.is_finite()
            || self.corner_radius < 0.0
            || !area.is_finite()
            || area == 0.0
    }

    pub(crate) fn bounds(self) -> Bounds {
        if self.is_empty() {
            return Bounds::new(0, 0, 0, 0);
        }
        let radius = f64::from(self.corner_radius);
        Bounds::new(
            (self.a.x.min(self.b.x).min(self.c.x) - radius).floor() as i32,
            (self.a.y.min(self.b.y).min(self.c.y) - radius).floor() as i32,
            (self.a.x.max(self.b.x).max(self.c.x) + radius).ceil() as i32,
            (self.a.y.max(self.b.y).max(self.c.y) + radius).ceil() as i32,
        )
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        for point in [&mut self.a, &mut self.b, &mut self.c] {
            point.x -= f64::from(dx);
            point.y -= f64::from(dy);
        }
        self
    }

    fn signed_double_area(self) -> f64 {
        (self.b.x - self.a.x) * (self.c.y - self.a.y)
            - (self.b.y - self.a.y) * (self.c.x - self.a.x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_triangles_remain_geometry_across_layout_scales() {
        for size in [1.0e-20, 1.0e-10, 1.0e-6, 1.0, 1.0e20] {
            for clockwise in [false, true] {
                let [b, c] = if clockwise {
                    [Point::new(size, 0.0), Point::new(0.0, size)]
                } else {
                    [Point::new(0.0, size), Point::new(size, 0.0)]
                };
                let triangle = Triangle::new(Point::ZERO, b, c, 0.0);
                assert!(!triangle.is_empty());
            }
        }
    }

    #[test]
    fn rounded_bounds_expand_all_three_vertices() {
        let triangle = Triangle::new(
            Point::new(8.0, 4.0),
            Point::new(20.0, 16.0),
            Point::new(8.0, 28.0),
            2.0,
        );

        assert_eq!(triangle.bounds(), Bounds::new(6, 2, 22, 30));
    }

    #[test]
    fn collinear_or_non_finite_triangle_is_empty() {
        let collinear = Triangle {
            a: Point::new(0.0, 0.0),
            b: Point::new(1.0, 1.0),
            c: Point::new(2.0, 2.0),
            corner_radius: 0.0,
        };
        let non_finite = Triangle {
            c: Point::new(f64::NAN, 2.0),
            ..collinear
        };

        assert!(collinear.is_empty());
        assert!(non_finite.is_empty());
    }
}
