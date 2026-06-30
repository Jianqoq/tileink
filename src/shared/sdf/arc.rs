use peniko::kurbo::Point;

use super::{
    coverage_from_dist,
    line::{LineCap, local_rect_distance},
    shadow::{ShadowOptions, shadow_alpha_from_distance, shadow_bounds},
};
use crate::{TILE_SIZE, shared::bounds::Bounds};

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

    pub(crate) fn signed_distance(self, x: f32, y: f32) -> f32 {
        let cx = self.center.x as f32;
        let cy = self.center.y as f32;
        let vx = x - cx;
        let vy = y - cy;
        let len = vx.hypot(vy);
        let half = self.width * 0.5;
        if self.is_full_sweep() {
            return (len - self.radius).abs() - half;
        }

        let body = self.butt_distance_from_local(vx, vy, len, half);
        match self.cap {
            LineCap::Butt => body,
            LineCap::Round => body
                .min(self.endpoint_distance(self.start_angle, vx, vy) - half)
                .min(self.endpoint_distance(self.start_angle + self.sweep_angle, vx, vy) - half),
            LineCap::Square => body
                .min(self.square_cap_distance(self.start_angle, vx, vy, -half, 0.0, half))
                .min(self.square_cap_distance(
                    self.start_angle + self.sweep_angle,
                    vx,
                    vy,
                    0.0,
                    half,
                    half,
                )),
        }
    }

    pub(crate) fn cap_value(self) -> f32 {
        self.cap as u32 as f32
    }

    fn butt_distance_from_local(self, vx: f32, vy: f32, len: f32, half: f32) -> f32 {
        if len <= ARC_EPSILON {
            return self
                .endpoint_distance(self.start_angle, vx, vy)
                .min(self.endpoint_distance(self.start_angle + self.sweep_angle, vx, vy))
                - half;
        }

        let angle = vy.atan2(vx);
        let radial = (len - self.radius).abs() - half;
        if self.angle_in_sweep(angle) {
            return radial;
        }

        self.cap_segment_distance(self.start_angle, vx, vy, half)
            .min(self.cap_segment_distance(self.start_angle + self.sweep_angle, vx, vy, half))
    }

    fn cap_segment_distance(self, angle: f32, vx: f32, vy: f32, half: f32) -> f32 {
        let inner_radius = (self.radius - half).max(0.0);
        let outer_radius = self.radius + half;
        let co = angle.cos();
        let si = angle.sin();
        distance_to_segment(
            vx,
            vy,
            inner_radius * co,
            inner_radius * si,
            outer_radius * co,
            outer_radius * si,
        )
    }

    fn square_cap_distance(self, angle: f32, vx: f32, vy: f32, x0: f32, x1: f32, half: f32) -> f32 {
        let dir = self.sweep_angle.signum();
        let co = angle.cos();
        let si = angle.sin();
        let ex = self.radius * co;
        let ey = self.radius * si;
        let tangent_x = -si * dir;
        let tangent_y = co * dir;
        let local_x = (vx - ex) * tangent_x + (vy - ey) * tangent_y;
        let local_y = -(vx - ex) * tangent_y + (vy - ey) * tangent_x;
        local_rect_distance(local_x, local_y, x0, x1, half)
    }

    fn endpoint_distance(self, angle: f32, vx: f32, vy: f32) -> f32 {
        let ex = self.radius * angle.cos();
        let ey = self.radius * angle.sin();
        (vx - ex).hypot(vy - ey)
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

    pub(crate) fn tile_is_solid(self, _: Bounds) -> bool {
        false
    }

    pub(crate) fn fine_area(
        self,
        area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize],
        tile_bounds: Bounds,
        pixel_bounds: Bounds,
    ) {
        let Some(options) = self.options.normalized() else {
            return;
        };

        for y_px in pixel_bounds.y0..pixel_bounds.y1 {
            let py = y_px as f32 + 0.5 - options.offset_y;
            let row = (y_px - tile_bounds.y0) as usize * TILE_SIZE as usize;
            for x_px in pixel_bounds.x0..pixel_bounds.x1 {
                let px = x_px as f32 + 0.5 - options.offset_x;
                area[row + (x_px - tile_bounds.x0) as usize] =
                    shadow_alpha_from_distance(self.arc.signed_distance(px, py), options);
            }
        }
    }
}

fn normalize_angle(angle: f32) -> f32 {
    angle - (angle / std::f32::consts::TAU).floor() * std::f32::consts::TAU
}

fn distance_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    if len2 <= ARC_EPSILON {
        return (px - ax).hypot(py - ay);
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
    (px - (ax + dx * t)).hypot(py - (ay + dy * t))
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

    #[test]
    fn butt_arc_does_not_render_beyond_cap() {
        let arc = Arc::new(
            Point::new(20.0, 20.0),
            10.0,
            0.0,
            std::f32::consts::FRAC_PI_2,
            4.0,
            LineCap::Butt,
        );

        assert!(arc.signed_distance(30.0, 20.0) < 0.0);
        assert!(arc.signed_distance(30.0, 14.0) > 0.0);
    }
}
