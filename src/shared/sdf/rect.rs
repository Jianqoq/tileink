use peniko::kurbo::Point;

use super::shadow::{ShadowOptions, shadow_bounds};
use crate::shared::bounds::Bounds;

pub type RectShadowOptions = ShadowOptions;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Radius {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_left: f32,
    pub bottom_right: f32,
}

impl Radius {
    pub const ZERO: Self = Self::all(0.0);

    pub const fn all(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_left: radius,
            bottom_right: radius,
        }
    }

    pub(crate) fn is_zero(self) -> bool {
        const EPS: f32 = 1e-6;
        self.top_left <= EPS
            && self.top_right <= EPS
            && self.bottom_left <= EPS
            && self.bottom_right <= EPS
    }

    fn offset_corners(
        self,
        top_left: f32,
        top_right: f32,
        bottom_left: f32,
        bottom_right: f32,
    ) -> Self {
        Self {
            top_left: (self.top_left + top_left).max(0.0),
            top_right: (self.top_right + top_right).max(0.0),
            bottom_left: (self.bottom_left + bottom_left).max(0.0),
            bottom_right: (self.bottom_right + bottom_right).max(0.0),
        }
    }
}

/// Per-side full stroke widths for centered rectangle strokes.
///
/// The SDF renderer expands the outer edge and shrinks the inner edge by half
/// of each side width, matching the existing `Stroke::width` centered-stroke
/// semantics while allowing each side to differ.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeWidths {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl StrokeWidths {
    pub fn all(width: f32) -> Self {
        Self {
            top: width,
            right: width,
            bottom: width,
            left: width,
        }
    }

    pub(crate) fn clamped(self) -> Self {
        Self {
            top: self.top.max(0.0),
            right: self.right.max(0.0),
            bottom: self.bottom.max(0.0),
            left: self.left.max(0.0),
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        let widths = self.clamped();
        widths.top == 0.0 && widths.right == 0.0 && widths.bottom == 0.0 && widths.left == 0.0
    }

    pub(crate) fn half(self) -> Self {
        let widths = self.clamped();
        Self {
            top: widths.top * 0.5,
            right: widths.right * 0.5,
            bottom: widths.bottom * 0.5,
            left: widths.left * 0.5,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub start: Point,
    pub end: Point,
    pub radius: Radius,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RectShadow {
    pub rect: Rect,
    pub options: RectShadowOptions,
}

impl Rect {
    pub(crate) fn bounds(&self) -> Bounds {
        let (x0, y0, x1, y1) = self.axis_bounds();
        Bounds::new(
            x0.floor() as i32,
            y0.floor() as i32,
            x1.ceil() as i32,
            y1.ceil() as i32,
        )
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.start.x -= f64::from(dx);
        self.start.y -= f64::from(dy);
        self.end.x -= f64::from(dx);
        self.end.y -= f64::from(dy);
        self
    }

    pub(crate) fn axis_bounds(&self) -> (f64, f64, f64, f64) {
        (
            self.start.x.min(self.end.x),
            self.start.y.min(self.end.y),
            self.start.x.max(self.end.x),
            self.start.y.max(self.end.y),
        )
    }
}

impl RectShadow {
    pub(crate) fn bounds(&self) -> Bounds {
        let (x0, y0, x1, y1) = self.rect.axis_bounds();
        shadow_bounds(
            Bounds::new(
                x0.floor() as i32,
                y0.floor() as i32,
                x1.ceil() as i32,
                y1.ceil() as i32,
            ),
            self.options,
        )
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.rect = self.rect.translated(dx, dy);
        self
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RectStroke {
    pub rect: Rect,
    /// Full side widths; rasterization derives inner and outer SDF bounds from
    /// half of these values so uniform strokes match `kurbo::Stroke::width`.
    pub widths: StrokeWidths,
}

impl RectStroke {
    pub(crate) fn bounds(&self) -> Bounds {
        self.outer_rect().bounds()
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.rect = self.rect.translated(dx, dy);
        self
    }

    fn outer_rect(&self) -> Rect {
        let (x0, y0, x1, y1) = self.rect.axis_bounds();
        let half = self.widths.half();
        Rect {
            start: Point::new(x0 - f64::from(half.left), y0 - f64::from(half.top)),
            end: Point::new(x1 + f64::from(half.right), y1 + f64::from(half.bottom)),
            radius: self.rect.radius.offset_corners(
                half.top.max(half.left),
                half.top.max(half.right),
                half.bottom.max(half.left),
                half.bottom.max(half.right),
            ),
        }
    }
}
