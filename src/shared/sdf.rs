use crate::shared::sdf::{
    arc::{Arc, ArcShadow},
    candlestick::CandleStick,
    circle::{Circle, CircleShadow, CircleStroke},
    line::{Line, LineShadow},
    rect::{Rect, RectShadow, RectStroke},
};
use crate::{TILE_SIZE, shared::bounds::Bounds};

pub mod arc;
pub mod candlestick;
pub mod circle;
pub mod line;
pub mod rect;
pub mod shadow;

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
    RectShadow(RectShadow),
    Circle(Circle),
    CircleStroke(CircleStroke),
    CircleShadow(CircleShadow),
    Arc(Arc),
    ArcShadow(ArcShadow),
    CandleStick(CandleStick),
    Line(Line),
    LineShadow(LineShadow),
}

impl Sdf {
    pub fn bounds(self) -> Bounds {
        match self {
            Self::Rect(rect) => rect.bounds(),
            Self::RectStroke(stroke) => stroke.bounds(),
            Self::RectShadow(shadow) => shadow.bounds(),
            Self::Circle(circle) => circle.bounds(),
            Self::CircleStroke(stroke) => stroke.bounds(),
            Self::CircleShadow(shadow) => shadow.bounds(),
            Self::Arc(arc) => arc.bounds(),
            Self::ArcShadow(shadow) => shadow.bounds(),
            Self::CandleStick(candle) => candle.bounds(),
            Self::Line(line) => line.bounds(),
            Self::LineShadow(shadow) => shadow.bounds(),
        }
    }

    pub(crate) fn translated(self, dx: f32, dy: f32) -> Self {
        match self {
            Self::Rect(rect) => Self::Rect(rect.translated(dx, dy)),
            Self::RectStroke(stroke) => Self::RectStroke(stroke.translated(dx, dy)),
            Self::RectShadow(shadow) => Self::RectShadow(shadow.translated(dx, dy)),
            Self::Circle(circle) => Self::Circle(circle.translated(dx, dy)),
            Self::CircleStroke(stroke) => Self::CircleStroke(stroke.translated(dx, dy)),
            Self::CircleShadow(shadow) => Self::CircleShadow(shadow.translated(dx, dy)),
            Self::Arc(arc) => Self::Arc(arc.translated(dx, dy)),
            Self::ArcShadow(shadow) => Self::ArcShadow(shadow.translated(dx, dy)),
            Self::CandleStick(candle) => Self::CandleStick(candle.translated(dx, dy)),
            Self::Line(line) => Self::Line(line.translated(dx, dy)),
            Self::LineShadow(shadow) => Self::LineShadow(shadow.translated(dx, dy)),
        }
    }

    pub(crate) fn tile_is_solid(&self, bounds: Bounds) -> bool {
        match self {
            Self::Rect(rect) => rect.tile_is_solid(bounds),
            Self::RectStroke(stroke) => stroke.tile_is_solid(bounds),
            Self::RectShadow(shadow) => shadow.tile_is_solid(bounds),
            Self::Circle(circle) => circle.tile_is_solid(bounds),
            Self::CircleStroke(stroke) => stroke.tile_is_solid(bounds),
            Self::CircleShadow(shadow) => shadow.tile_is_solid(bounds),
            Self::Arc(arc) => arc.tile_is_solid(bounds),
            Self::ArcShadow(shadow) => shadow.tile_is_solid(bounds),
            Self::CandleStick(candle) => candle.tile_is_solid(bounds),
            Self::Line(line) => line.tile_is_solid(bounds),
            Self::LineShadow(shadow) => shadow.tile_is_solid(bounds),
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
            Self::RectShadow(shadow) => shadow.fine_area(area, tile_bounds, pixel_bounds),
            Self::Circle(circle) => circle.fine_area(area, tile_bounds, pixel_bounds),
            Self::CircleStroke(stroke) => stroke.fine_area(area, tile_bounds, pixel_bounds),
            Self::CircleShadow(shadow) => shadow.fine_area(area, tile_bounds, pixel_bounds),
            Self::Arc(arc) => arc.fine_area(area, tile_bounds, pixel_bounds),
            Self::ArcShadow(shadow) => shadow.fine_area(area, tile_bounds, pixel_bounds),
            Self::CandleStick(candle) => candle.fine_area(area, tile_bounds, pixel_bounds),
            Self::Line(line) => line.fine_area(area, tile_bounds, pixel_bounds),
            Self::LineShadow(shadow) => shadow.fine_area(area, tile_bounds, pixel_bounds),
        }
    }
}

#[cfg(test)]
mod tests {
    use peniko::kurbo::Point;

    use super::*;
    use crate::shared::sdf::rect::Radius;

    #[test]
    fn translated_shifts_sdf_bounds_into_local_space() {
        let sdf = Sdf::Rect(Rect {
            start: Point::new(12.0, 24.0),
            end: Point::new(28.0, 40.0),
            radius: Radius::all(3.0),
        });

        assert_eq!(
            sdf.translated(10.0, 20.0).bounds(),
            Bounds::new(2, 4, 18, 20)
        );
    }
}
