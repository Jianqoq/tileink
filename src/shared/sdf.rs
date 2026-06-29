use crate::shared::sdf::{
    candlestick::CandleStick,
    circle::{Circle, CircleStroke},
    line::Line,
    rect::{Rect, RectStroke},
};
use crate::{TILE_SIZE, shared::bounds::Bounds};

pub mod candlestick;
pub mod circle;
pub mod line;
pub mod rect;

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
    CircleStroke(CircleStroke),
    CandleStick(CandleStick),
    Line(Line),
}

impl Sdf {
    pub(crate) fn tile_is_solid(&self, bounds: Bounds) -> bool {
        match self {
            Self::Rect(rect) => rect.tile_is_solid(bounds),
            Self::RectStroke(stroke) => stroke.tile_is_solid(bounds),
            Self::Circle(circle) => circle.tile_is_solid(bounds),
            Self::CircleStroke(stroke) => stroke.tile_is_solid(bounds),
            Self::CandleStick(candle) => candle.tile_is_solid(bounds),
            Self::Line(line) => line.tile_is_solid(bounds),
        }
    }

    pub(crate) fn fine_area(
        &self,
        area: &mut [f32; (TILE_SIZE * TILE_SIZE) as usize],
        tile_bounds: Bounds,
        pixel_bounds: Bounds,
    ) {
        match self {
            Self::Rect(rect) => rect.fine_area(area, tile_bounds, pixel_bounds),
            Self::RectStroke(stroke) => stroke.fine_area(area, tile_bounds, pixel_bounds),
            Self::Circle(circle) => circle.fine_area(area, tile_bounds, pixel_bounds),
            Self::CircleStroke(stroke) => stroke.fine_area(area, tile_bounds, pixel_bounds),
            Self::CandleStick(candle) => candle.fine_area(area, tile_bounds, pixel_bounds),
            Self::Line(line) => line.fine_area(area, tile_bounds, pixel_bounds),
        }
    }
}
