use crate::shared::bounds::Bounds;
use crate::shared::sdf::{
    arc::{ArcShadow, Rc},
    candlestick::CandleStick,
    checkerboard::Checkerboard,
    circle::{Circle, CircleShadow, CircleStroke},
    line::{DashLine, Line, LineShadow},
    rect::{Rect, RectShadow, RectStroke},
};

pub mod arc;
pub mod candlestick;
pub mod checkerboard;
pub mod circle;
pub mod line;
pub mod rect;
pub mod shadow;
pub mod triangle;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum Sdf {
    Rect(Rect),
    RectStroke(RectStroke),
    Circle(Circle),
    CircleStroke(CircleStroke),
    Rc(Rc),
    CandleStick(CandleStick),
    Line(Line),
    DashLine(DashLine),
    Triangle(triangle::Triangle),
    Checkerboard(Checkerboard),
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum SdfShadow {
    Rect(RectShadow),
    Circle(CircleShadow),
    Rc(ArcShadow),
    Line(LineShadow),
}

impl Sdf {
    /// Creates analytic coverage for alternating cells inside `rect`.
    pub fn checkerboard(rect: peniko::kurbo::Rect, cell_size: f32) -> Self {
        Self::Checkerboard(Checkerboard::new(rect, cell_size))
    }

    pub fn bounds(self) -> Bounds {
        match self {
            Self::Rect(rect) => rect.bounds(),
            Self::RectStroke(stroke) => stroke.bounds(),
            Self::Circle(circle) => circle.bounds(),
            Self::CircleStroke(stroke) => stroke.bounds(),
            Self::Rc(arc) => arc.bounds(),
            Self::CandleStick(candle) => candle.bounds(),
            Self::Line(line) => line.bounds(),
            Self::DashLine(line) => line.bounds(),
            Self::Triangle(triangle) => triangle.bounds(),
            Self::Checkerboard(checkerboard) => checkerboard.bounds(),
        }
    }

    pub(crate) fn translated(self, dx: f32, dy: f32) -> Self {
        match self {
            Self::Rect(rect) => Self::Rect(rect.translated(dx, dy)),
            Self::RectStroke(stroke) => Self::RectStroke(stroke.translated(dx, dy)),
            Self::Circle(circle) => Self::Circle(circle.translated(dx, dy)),
            Self::CircleStroke(stroke) => Self::CircleStroke(stroke.translated(dx, dy)),
            Self::Rc(arc) => Self::Rc(arc.translated(dx, dy)),
            Self::CandleStick(candle) => Self::CandleStick(candle.translated(dx, dy)),
            Self::Line(line) => Self::Line(line.translated(dx, dy)),
            Self::DashLine(line) => Self::DashLine(line.translated(dx, dy)),
            Self::Triangle(triangle) => Self::Triangle(triangle.translated(dx, dy)),
            Self::Checkerboard(checkerboard) => Self::Checkerboard(checkerboard.translated(dx, dy)),
        }
    }
}

impl SdfShadow {
    pub fn bounds(self) -> Bounds {
        match self {
            Self::Rect(shadow) => shadow.bounds(),
            Self::Circle(shadow) => shadow.bounds(),
            Self::Rc(shadow) => shadow.bounds(),
            Self::Line(shadow) => shadow.bounds(),
        }
    }

    pub(crate) fn translated(self, dx: f32, dy: f32) -> Self {
        match self {
            Self::Rect(shadow) => Self::Rect(shadow.translated(dx, dy)),
            Self::Circle(shadow) => Self::Circle(shadow.translated(dx, dy)),
            Self::Rc(shadow) => Self::Rc(shadow.translated(dx, dy)),
            Self::Line(shadow) => Self::Line(shadow.translated(dx, dy)),
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
