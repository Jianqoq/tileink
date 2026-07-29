use peniko::kurbo::Point;

use crate::shared::bounds::Bounds;

const POINT_COUNT: usize = 10;

/// Filled five-point star with analytic rounded corners.
///
/// The boundary is the non-self-intersecting polygon formed by alternating outer and inner
/// vertices. `corner_radius` expands that polygon with a circular Minkowski radius, which rounds
/// every convex and concave corner without flattening the star into path segments.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Star {
    pub center: Point,
    pub outer_radius: f32,
    pub inner_radius: f32,
    pub corner_radius: f32,
    pub rotation_radians: f32,
}

impl Star {
    pub fn new(
        center: Point,
        outer_radius: f32,
        inner_radius: f32,
        corner_radius: f32,
        rotation_radians: f32,
    ) -> Self {
        let star = Self {
            center,
            outer_radius,
            inner_radius,
            corner_radius,
            rotation_radians,
        };
        assert!(
            !star.is_empty(),
            "SDF star center and rotation must be finite, radii must be finite and positive, inner radius must be smaller than outer radius, and corner radius must be non-negative"
        );
        star
    }

    pub(crate) fn is_empty(self) -> bool {
        !self.center.x.is_finite()
            || !self.center.y.is_finite()
            || !self.outer_radius.is_finite()
            || self.outer_radius <= 0.0
            || !self.inner_radius.is_finite()
            || self.inner_radius <= 0.0
            || self.inner_radius >= self.outer_radius
            || !self.corner_radius.is_finite()
            || self.corner_radius < 0.0
            || !self.rotation_radians.is_finite()
    }

    pub(crate) fn bounds(self) -> Bounds {
        self.expanded_bounds(0.0)
    }

    fn expanded_bounds(self, extra: f32) -> Bounds {
        if self.is_empty() {
            return Bounds::new(0, 0, 0, 0);
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for index in 0..POINT_COUNT {
            let vertex = self.vertex(index);
            min_x = min_x.min(vertex.x);
            min_y = min_y.min(vertex.y);
            max_x = max_x.max(vertex.x);
            max_y = max_y.max(vertex.y);
        }
        let expansion = f64::from(self.corner_radius) + f64::from(extra);
        Bounds::new(
            (min_x - expansion).floor() as i32,
            (min_y - expansion).floor() as i32,
            (max_x + expansion).ceil() as i32,
            (max_y + expansion).ceil() as i32,
        )
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.center.x -= f64::from(dx);
        self.center.y -= f64::from(dy);
        self
    }

    fn vertex(self, index: usize) -> Point {
        let radius = if index.is_multiple_of(2) {
            self.outer_radius
        } else {
            self.inner_radius
        };
        let angle = f64::from(self.rotation_radians) + index as f64 * std::f64::consts::PI / 5.0;
        Point::new(
            self.center.x + f64::from(radius) * angle.cos(),
            self.center.y + f64::from(radius) * angle.sin(),
        )
    }
}

/// Centered stroke around a rounded [`Star`] boundary.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StarStroke {
    pub star: Star,
    pub half_width: f32,
}

impl StarStroke {
    pub fn new(star: Star, width: f32) -> Self {
        let stroke = Self {
            star,
            half_width: width * 0.5,
        };
        assert!(
            !stroke.is_empty(),
            "SDF star stroke width must be finite and positive"
        );
        stroke
    }

    pub(crate) fn is_empty(self) -> bool {
        self.star.is_empty() || !self.half_width.is_finite() || self.half_width <= 0.0
    }

    pub(crate) fn bounds(self) -> Bounds {
        if self.is_empty() {
            return Bounds::new(0, 0, 0, 0);
        }
        self.star.expanded_bounds(self.half_width)
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.star = self.star.translated(dx, dy);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_star_bounds_include_rounded_outer_vertices() {
        let star = Star::new(
            Point::new(16.0, 20.0),
            8.0,
            3.5,
            1.25,
            -std::f32::consts::FRAC_PI_2,
        );

        assert_eq!(star.bounds(), Bounds::new(7, 10, 25, 28));
        assert_eq!(
            StarStroke::new(star, 2.5).bounds(),
            Bounds::new(5, 9, 27, 29)
        );
    }

    #[test]
    fn invalid_public_literals_are_empty() {
        let star = Star {
            center: Point::ZERO,
            outer_radius: 8.0,
            inner_radius: 8.0,
            corner_radius: 0.0,
            rotation_radians: 0.0,
        };
        assert!(star.is_empty());
        assert!(
            StarStroke {
                star: Star {
                    inner_radius: 3.0,
                    ..star
                },
                half_width: f32::NAN,
            }
            .is_empty()
        );
    }

    #[test]
    fn translation_preserves_geometry_and_moves_center() {
        let star = Star::new(Point::new(18.0, 19.0), 8.0, 3.5, 1.0, 0.3);
        let translated = star.translated(4.0, 6.0);

        assert_eq!(translated.center, Point::new(14.0, 13.0));
        assert_eq!(translated.outer_radius, star.outer_radius);
        assert_eq!(translated.rotation_radians, star.rotation_radians);
    }
}
