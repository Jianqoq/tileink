use peniko::kurbo::Point;

use super::{
    line::LineCap,
    shadow::{ShadowOptions, shadow_bounds},
};
use crate::shared::bounds::Bounds;

const ARC_EPSILON: f32 = 1.0e-6;
const ARC_FULL_SWEEP_EPSILON: f32 = 1.0e-4;

/// Circular stroked arc SDF.
///
/// Angles are radians in device coordinates, using `atan2(y - cy, x - cx)`.
/// Positive sweep follows increasing `atan2` values, which is clockwise on the
/// usual screen coordinate system where y grows downward.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    pub center: Point,
    pub radius: f32,
    pub start_angle: f32,
    pub sweep_angle: f32,
    pub width: f32,
    pub cap: LineCap,
}

impl Arc {
    pub fn new(
        center: Point,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        width: f32,
        cap: LineCap,
    ) -> Self {
        assert!(radius > 0.0, "SDF arc radius must be positive");
        assert!(width > 0.0, "SDF arc width must be positive");
        Self {
            center,
            radius,
            start_angle,
            sweep_angle,
            width,
            cap,
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.radius <= 0.0 || self.width <= 0.0 || self.sweep_angle.abs() <= ARC_EPSILON
    }

    pub(crate) fn is_full_sweep(self) -> bool {
        self.sweep_angle.abs() >= std::f32::consts::TAU - ARC_FULL_SWEEP_EPSILON
    }

    pub(crate) fn bounds(self) -> Bounds {
        if self.is_empty() {
            return Bounds::new(0, 0, 0, 0);
        }

        let cx = self.center.x as f32;
        let cy = self.center.y as f32;
        let half = self.width * 0.5;
        if self.is_full_sweep() {
            let r = self.radius + half;
            return Bounds::new(
                (cx - r).floor() as i32,
                (cy - r).floor() as i32,
                (cx + r).ceil() as i32,
                (cy + r).ceil() as i32,
            );
        }

        let mut x0 = f32::INFINITY;
        let mut y0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        let mut y1 = f32::NEG_INFINITY;
        self.include_point_at_angle(&mut x0, &mut y0, &mut x1, &mut y1, self.start_angle);
        self.include_point_at_angle(
            &mut x0,
            &mut y0,
            &mut x1,
            &mut y1,
            self.start_angle + self.sweep_angle,
        );
        for angle in [
            0.0,
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
            std::f32::consts::PI + std::f32::consts::FRAC_PI_2,
        ] {
            if self.angle_in_sweep(angle) {
                self.include_point_at_angle(&mut x0, &mut y0, &mut x1, &mut y1, angle);
            }
        }

        let outset = half + 1.0;
        Bounds::new(
            (x0 - outset).floor() as i32,
            (y0 - outset).floor() as i32,
            (x1 + outset).ceil() as i32,
            (y1 + outset).ceil() as i32,
        )
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.center.x -= f64::from(dx);
        self.center.y -= f64::from(dy);
        self
    }

    pub(crate) fn cap_value(self) -> f32 {
        self.cap as u32 as f32
    }

    fn include_point_at_angle(
        self,
        x0: &mut f32,
        y0: &mut f32,
        x1: &mut f32,
        y1: &mut f32,
        angle: f32,
    ) {
        let x = self.center.x as f32 + self.radius * angle.cos();
        let y = self.center.y as f32 + self.radius * angle.sin();
        *x0 = x0.min(x);
        *y0 = y0.min(y);
        *x1 = x1.max(x);
        *y1 = y1.max(y);
    }

    fn angle_in_sweep(self, angle: f32) -> bool {
        if self.is_full_sweep() {
            return true;
        }

        if self.sweep_angle >= 0.0 {
            normalize_angle(angle - self.start_angle) <= self.sweep_angle + ARC_EPSILON
        } else {
            normalize_angle(self.start_angle - angle) <= -self.sweep_angle + ARC_EPSILON
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ArcShadow {
    pub arc: Arc,
    pub options: ShadowOptions,
}

impl ArcShadow {
    pub(crate) fn bounds(self) -> Bounds {
        shadow_bounds(self.arc.bounds(), self.options)
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.arc = self.arc.translated(dx, dy);
        self
    }
}

fn normalize_angle(angle: f32) -> f32 {
    angle - (angle / std::f32::consts::TAU).floor() * std::f32::consts::TAU
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_bounds_include_endpoints_quadrants_and_stroke_width() {
        let arc = Arc::new(
            Point::new(20.0, 20.0),
            10.0,
            0.0,
            std::f32::consts::PI,
            4.0,
            LineCap::Butt,
        );

        assert_eq!(arc.bounds(), Bounds::new(7, 17, 33, 33));
    }
}
