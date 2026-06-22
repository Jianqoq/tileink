use crate::shared::sdf::{
    circle::Circle,
    rect::{Rect, RectStroke},
};

pub mod circle;
pub mod rect;
pub mod rect_stroke;

pub(super) const SOLID_DIST: f64 = -0.5;

#[inline]
pub(super) fn coverage_from_dist(dist: f32) -> f32 {
    (0.5 - dist).clamp(0.0, 1.0)
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum Sdf {
    Rect(Rect),
    RectStroke(RectStroke),
    Circle(Circle),
}
