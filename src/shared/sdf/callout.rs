use peniko::kurbo::{Point, Rect};

use crate::shared::{bounds::Bounds, sdf::shadow::ShadowOptions};

/// Edge of a callout body from which its tail extends.
#[repr(u32)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CalloutSide {
    Top,
    Right,
    #[default]
    Bottom,
    Left,
}

impl CalloutSide {
    pub(crate) const fn value(self) -> f32 {
        self as u32 as f32
    }

    pub(crate) const fn from_value(value: f32) -> Self {
        match value as u32 {
            value if value == Self::Top as u32 => Self::Top,
            value if value == Self::Right as u32 => Self::Right,
            value if value == Self::Left as u32 => Self::Left,
            _ => Self::Bottom,
        }
    }
}

/// Tail geometry relative to the callout body's minimum X or Y edge.
///
/// `offset` is the tail center measured from the body's left edge for top/bottom tails and from
/// its top edge for left/right tails. Rendering clamps it to the available edge without changing
/// the authored value, so retained geometry remains stable when a responsive caller is briefly
/// narrower than its preferred tail metrics.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalloutTail {
    pub visible: bool,
    pub side: CalloutSide,
    pub offset: f32,
    pub width: f32,
    pub length: f32,
    pub radius: f32,
}

impl CalloutTail {
    pub fn new(side: CalloutSide, offset: f32, width: f32, length: f32, radius: f32) -> Self {
        let tail = Self {
            visible: true,
            side,
            offset,
            width,
            length,
            radius,
        };
        assert!(
            !tail.is_empty(),
            "SDF callout tail metrics must be finite, width and length must be positive, and radius must be non-negative"
        );
        tail
    }

    /// Hide the tail while preserving the callout body, stroke, and shadow semantics.
    pub const fn hidden() -> Self {
        Self {
            visible: false,
            side: CalloutSide::Bottom,
            offset: 0.0,
            width: 0.0,
            length: 0.0,
            radius: 0.0,
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.visible
            && (!self.offset.is_finite()
                || !self.width.is_finite()
                || self.width <= 0.0
                || !self.length.is_finite()
                || self.length <= 0.0
                || !self.radius.is_finite()
                || self.radius < 0.0)
    }
}

/// Filled rounded rectangle and directional tail represented by one analytic SDF union.
///
/// The body uses one uniform radius because tooltip/callout chrome is a compact semantic shape.
/// More general independently-rounded panels remain [`crate::SdfRect`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Callout {
    pub start: Point,
    pub end: Point,
    pub body_radius: f32,
    pub tail: CalloutTail,
}

impl Callout {
    pub fn new(body: Rect, body_radius: f32, tail: CalloutTail) -> Self {
        let callout = Self {
            start: Point::new(body.x0, body.y0),
            end: Point::new(body.x1, body.y1),
            body_radius,
            tail,
        };
        assert!(
            !callout.is_empty(),
            "SDF callout body must be finite and non-empty, body radius must be finite and non-negative, and tail geometry must be valid"
        );
        callout
    }

    pub(crate) fn is_empty(self) -> bool {
        let (x0, y0, x1, y1) = self.axis_bounds();
        ![x0, y0, x1, y1].into_iter().all(f64::is_finite)
            || x0 >= x1
            || y0 >= y1
            || !self.body_radius.is_finite()
            || self.body_radius < 0.0
            || self.tail.is_empty()
    }

    pub(crate) fn axis_bounds(self) -> (f64, f64, f64, f64) {
        (
            self.start.x.min(self.end.x),
            self.start.y.min(self.end.y),
            self.start.x.max(self.end.x),
            self.start.y.max(self.end.y),
        )
    }

    pub(crate) fn bounds(self) -> Bounds {
        self.expanded_bounds(0.0)
    }

    fn expanded_bounds(self, extra: f32) -> Bounds {
        if self.is_empty() {
            return Bounds::new(0, 0, 0, 0);
        }
        let (mut x0, mut y0, mut x1, mut y1) = self.axis_bounds();
        if self.tail.visible {
            let tail = f64::from(self.tail.length);
            match self.tail.side {
                CalloutSide::Top => y0 -= tail,
                CalloutSide::Right => x1 += tail,
                CalloutSide::Bottom => y1 += tail,
                CalloutSide::Left => x0 -= tail,
            }
        }
        let extra = f64::from(extra.max(0.0));
        Bounds::new(
            (x0 - extra).floor() as i32,
            (y0 - extra).floor() as i32,
            (x1 + extra).ceil() as i32,
            (y1 + extra).ceil() as i32,
        )
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.start.x -= f64::from(dx);
        self.start.y -= f64::from(dy);
        self.end.x -= f64::from(dx);
        self.end.y -= f64::from(dy);
        self
    }

    pub(crate) fn physical(mut self, scale: f32) -> Self {
        self.start.x *= f64::from(scale);
        self.start.y *= f64::from(scale);
        self.end.x *= f64::from(scale);
        self.end.y *= f64::from(scale);
        self.body_radius *= scale;
        self.tail.offset *= scale;
        self.tail.width *= scale;
        self.tail.length *= scale;
        self.tail.radius *= scale;
        self
    }
}

/// Centered stroke around the complete callout union.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalloutStroke {
    pub callout: Callout,
    pub half_width: f32,
}

impl CalloutStroke {
    pub fn new(callout: Callout, width: f32) -> Self {
        let stroke = Self {
            callout,
            half_width: width * 0.5,
        };
        assert!(
            !stroke.is_empty(),
            "SDF callout stroke width must be finite and positive"
        );
        stroke
    }

    pub(crate) fn is_empty(self) -> bool {
        self.callout.is_empty() || !self.half_width.is_finite() || self.half_width <= 0.0
    }

    pub(crate) fn bounds(self) -> Bounds {
        self.callout.expanded_bounds(self.half_width)
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.callout = self.callout.translated(dx, dy);
        self
    }

    pub(crate) fn physical(mut self, scale: f32) -> Self {
        self.callout = self.callout.physical(scale);
        self.half_width *= scale;
        self
    }
}

/// Soft shadow around the complete callout union.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalloutShadow {
    pub callout: Callout,
    pub options: ShadowOptions,
}

impl CalloutShadow {
    pub(crate) fn bounds(self) -> Bounds {
        crate::shared::sdf::shadow::shadow_bounds(self.callout.bounds(), self.options)
    }

    pub(crate) fn translated(mut self, dx: f32, dy: f32) -> Self {
        self.callout = self.callout.translated(dx, dy);
        self
    }

    pub(crate) fn physical(mut self, scale: f32) -> Self {
        self.callout = self.callout.physical(scale);
        self.options.offset_x *= scale;
        self.options.offset_y *= scale;
        self.options.expand *= scale;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn callout(side: CalloutSide) -> Callout {
        Callout::new(
            Rect::new(10.0, 20.0, 110.0, 60.0),
            8.0,
            CalloutTail::new(side, 50.0, 10.0, 6.0, 1.5),
        )
    }

    #[test]
    fn bounds_include_each_tail_direction() {
        assert_eq!(
            callout(CalloutSide::Top).bounds(),
            Bounds::new(10, 14, 110, 60)
        );
        assert_eq!(
            callout(CalloutSide::Right).bounds(),
            Bounds::new(10, 20, 116, 60)
        );
        assert_eq!(
            callout(CalloutSide::Bottom).bounds(),
            Bounds::new(10, 20, 110, 66)
        );
        assert_eq!(
            callout(CalloutSide::Left).bounds(),
            Bounds::new(4, 20, 110, 60)
        );
    }

    #[test]
    fn translation_preserves_relative_tail_offset() {
        let translated = callout(CalloutSide::Bottom).translated(4.0, 6.0);
        assert_eq!(translated.start, Point::new(6.0, 14.0));
        assert_eq!(translated.end, Point::new(106.0, 54.0));
        assert_eq!(translated.tail.offset, 50.0);
    }

    #[test]
    fn invalid_struct_literals_are_empty() {
        let mut invalid = callout(CalloutSide::Bottom);
        invalid.tail.length = f32::NAN;
        assert!(invalid.is_empty());

        invalid = callout(CalloutSide::Bottom);
        invalid.end.x = invalid.start.x;
        assert!(invalid.is_empty());
    }

    #[test]
    fn stroke_and_shadow_expand_the_complete_union() {
        let callout = callout(CalloutSide::Bottom);
        assert_eq!(
            CalloutStroke::new(callout, 2.0).bounds(),
            Bounds::new(9, 19, 111, 67)
        );
        let shadow = CalloutShadow {
            callout,
            options: ShadowOptions::new(0.0, 2.0, 4.0, 0.5),
        };
        assert_eq!(shadow.bounds(), Bounds::new(-7, 5, 127, 85));
    }

    #[test]
    fn hidden_tail_reduces_fill_stroke_and_shadow_to_the_body() {
        let mut callout = callout(CalloutSide::Bottom);
        callout.tail = CalloutTail::hidden();

        assert_eq!(callout.bounds(), Bounds::new(10, 20, 110, 60));
        assert_eq!(
            CalloutStroke::new(callout, 2.0).bounds(),
            Bounds::new(9, 19, 111, 61)
        );
    }
}
