//! 2D vector math for Euler spiral flattening (ported from Vello `vello_shaders`).

use std::ops::{Add, Div, Mul, Neg, Sub};

use crate::shared::cubic::CubicPoints;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub(crate) struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;
    fn mul(self, rhs: Vec2) -> Self::Output {
        rhs * self
    }
}

impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self::new(-self.x, -self.y)
    }
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    pub fn mix(self, other: Self, t: f32) -> Self {
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }

    pub fn atan2(self) -> f32 {
        self.y.atan2(self.x)
    }

    pub fn line_to_cubic(p0: Vec2, p1: Vec2) -> CubicPoints {
        let p3 = p1;
        let p2 = p3.mix(p0, 1.0 / 3.0);
        let p1 = p0.mix(p3, 1.0 / 3.0);
        CubicPoints { p0, p1, p2, p3 }
    }

    pub fn quad_to_cubic(p0: Vec2, p1: Vec2, p2: Vec2) -> CubicPoints {
        let p3 = p2;
        let p2 = p1.mix(p2, 1.0 / 3.0);
        let p1 = p1.mix(p0, 1.0 / 3.0);
        CubicPoints { p0, p1, p2, p3 }
    }
}
