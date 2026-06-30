use peniko::kurbo::Point;

use super::{
    coverage_from_dist,
    shadow::{ShadowOptions, shadow_alpha_from_distance, shadow_bounds},
};
use crate::{TILE_SIZE, shared::bounds::Bounds};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Circle {
    pub center: Point,
    pub radius: f32,
}

impl Circle {
    pub(crate) fn bounds(self) -> Bounds {
        Bounds::new(
            (self.center.x as f32 - self.radius).floor() as i32,
            (self.center.y as f32 - self.radius).floor() as i32,
            (self.center.x as f32 + self.radius).ceil() as i32,
            (self.center.y as f32 + self.radius).ceil() as i32,
        )
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.center.x -= f64::from(dx);
        self.center.y -= f64::from(dy);
        self
    }

    pub(crate) fn tile_is_solid(&self, bounds: Bounds) -> bool {
        if bounds.x0 >= bounds.x1 || bounds.y0 >= bounds.y1 {
            return false;
        }

        let inner_r = f64::from(self.radius) - 0.5;
        if inner_r <= 0.0 {
            return false;
        }

        let cx = self.center.x;
        let cy = self.center.y;
        let r2 = inner_r * inner_r;
        let corners = [
            (bounds.x0 as f64 + 0.5, bounds.y0 as f64 + 0.5),
            (bounds.x1 as f64 - 0.5, bounds.y0 as f64 + 0.5),
            (bounds.x0 as f64 + 0.5, bounds.y1 as f64 - 0.5),
            (bounds.x1 as f64 - 0.5, bounds.y1 as f64 - 0.5),
        ];
        corners.iter().all(|&(x, y)| {
            let dx = x - cx;
            let dy = y - cy;
            dx * dx + dy * dy <= r2
        })
    }

    pub(crate) fn fine_area(
        &self,
        area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize],
        tile_bounds: Bounds,
        pixel_bounds: Bounds,
    ) {
        let cx = self.center.x as f32;
        let cy = self.center.y as f32;
        let r = self.radius;
        let r_out = r + 0.5;
        let r_out2 = r_out * r_out;
        let r_in = (r - 0.5).max(0.0);
        let r_in2 = r_in * r_in;
        let tile_x0 = tile_bounds.x0;
        let tile_y0 = tile_bounds.y0;
        let stride = TILE_SIZE as usize;

        for y_px in pixel_bounds.y0..pixel_bounds.y1 {
            let py = y_px as f32 + 0.5;
            let dy = py - cy;
            let dy2 = dy * dy;
            if dy2 > r_out2 {
                continue;
            }
            let half_w_out = (r_out2 - dy2).sqrt();
            let half_w_in = if dy2 < r_in2 {
                (r_in2 - dy2).sqrt()
            } else {
                0.0
            };
            let row = (y_px - tile_y0) as usize * stride;

            for x_px in pixel_bounds.x0..pixel_bounds.x1 {
                let px = x_px as f32 + 0.5;
                let dx = px - cx;
                let dx_abs = dx.abs();
                let ix = row + (x_px - tile_x0) as usize;
                if dx_abs > half_w_out {
                    area[ix] = 0.0;
                } else if dx_abs <= half_w_in {
                    area[ix] = 1.0;
                } else {
                    let dist = (dx * dx + dy2).sqrt() - r;
                    area[ix] = coverage_from_dist(dist);
                }
            }
        }
    }

    pub(crate) fn signed_distance(self, x: f32, y: f32) -> f32 {
        let dx = x - self.center.x as f32;
        let dy = y - self.center.y as f32;
        dx.hypot(dy) - self.radius
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CircleShadow {
    pub circle: Circle,
    pub options: ShadowOptions,
}

impl CircleShadow {
    pub(crate) fn bounds(self) -> Bounds {
        shadow_bounds(self.circle.bounds(), self.options)
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.circle = self.circle.translated(dx, dy);
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
                    shadow_alpha_from_distance(self.circle.signed_distance(px, py), options);
            }
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CircleStroke {
    pub circle: Circle,
    pub half_width: f32,
}

impl CircleStroke {
    pub(crate) fn tile_is_solid(&self, _: Bounds) -> bool {
        false
    }

    pub(crate) fn fine_area(
        &self,
        area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize],
        tile_bounds: Bounds,
        pixel_bounds: Bounds,
    ) {
        if self.half_width <= 0.0 {
            return;
        }

        Circle {
            center: self.circle.center,
            radius: self.circle.radius + self.half_width,
        }
        .fine_area(area, tile_bounds, pixel_bounds);

        let inner_radius = self.circle.radius - self.half_width;
        if inner_radius <= 0.0 {
            return;
        }

        let mut inner_area = [0.0; (TILE_SIZE * TILE_SIZE) as usize];
        Circle {
            center: self.circle.center,
            radius: inner_radius,
        }
        .fine_area(&mut inner_area, tile_bounds, pixel_bounds);
        for (outer, inner) in area.iter_mut().zip(inner_area) {
            *outer = (*outer - inner).clamp(0.0, 1.0);
        }
    }
}
