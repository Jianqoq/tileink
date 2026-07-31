use peniko::kurbo::Point;

use super::shadow::{ShadowOptions, shadow_bounds};
use crate::shared::bounds::Bounds;

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
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CircleStroke {
    pub circle: Circle,
    pub half_width: f32,
}

impl CircleStroke {
    pub(crate) fn bounds(self) -> Bounds {
        Circle {
            center: self.circle.center,
            radius: self.circle.radius + self.half_width.max(0.0),
        }
        .bounds()
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.circle = self.circle.translated(dx, dy);
        self
    }
}
