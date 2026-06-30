use crate::{
    cubecl::types::{
        CUBE_SDF_ARC, CUBE_SDF_ARC_SHADOW, CUBE_SDF_CANDLESTICK, CUBE_SDF_CIRCLE,
        CUBE_SDF_CIRCLE_SHADOW, CUBE_SDF_CIRCLE_STROKE, CUBE_SDF_LINE, CUBE_SDF_LINE_SHADOW,
        CUBE_SDF_NONE, CUBE_SDF_RECT, CUBE_SDF_RECT_SHADOW, CUBE_SDF_RECT_STROKE,
    },
    shared::sdf::Sdf,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EncodedSdf {
    pub(crate) kind: u32,
    pub(crate) coords: [f32; 4],
    pub(crate) radii: [f32; 4],
    pub(crate) stroke: [f32; 4],
    pub(crate) shadow: [f32; 4],
}

impl EncodedSdf {
    pub(crate) const NONE: Self = Self {
        kind: CUBE_SDF_NONE,
        coords: [0.0; 4],
        radii: [0.0; 4],
        stroke: [0.0; 4],
        shadow: [0.0; 4],
    };
}

pub(crate) fn encode_sdf(sdf: Sdf) -> EncodedSdf {
    match sdf {
        Sdf::Rect(rect) => {
            let (x0, y0, x1, y1) = rect.axis_bounds();
            EncodedSdf {
                kind: CUBE_SDF_RECT,
                coords: [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
                radii: [
                    rect.radius.top_left,
                    rect.radius.top_right,
                    rect.radius.bottom_left,
                    rect.radius.bottom_right,
                ],
                ..EncodedSdf::NONE
            }
        }
        Sdf::RectStroke(stroke) => {
            let (x0, y0, x1, y1) = stroke.rect.axis_bounds();
            let half = stroke.widths.half();
            EncodedSdf {
                kind: CUBE_SDF_RECT_STROKE,
                coords: [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
                radii: [
                    stroke.rect.radius.top_left,
                    stroke.rect.radius.top_right,
                    stroke.rect.radius.bottom_left,
                    stroke.rect.radius.bottom_right,
                ],
                stroke: [half.top, half.right, half.bottom, half.left],
                ..EncodedSdf::NONE
            }
        }
        Sdf::RectShadow(shadow) => {
            let (x0, y0, x1, y1) = shadow.rect.axis_bounds();
            EncodedSdf {
                kind: CUBE_SDF_RECT_SHADOW,
                coords: [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
                radii: [
                    shadow.rect.radius.top_left,
                    shadow.rect.radius.top_right,
                    shadow.rect.radius.bottom_left,
                    shadow.rect.radius.bottom_right,
                ],
                shadow: [
                    shadow.options.offset_x,
                    shadow.options.offset_y,
                    shadow.options.expand,
                    shadow.options.intensity,
                ],
                ..EncodedSdf::NONE
            }
        }
        Sdf::Circle(circle) => EncodedSdf {
            kind: CUBE_SDF_CIRCLE,
            coords: [
                circle.center.x as f32,
                circle.center.y as f32,
                circle.radius,
                0.0,
            ],
            ..EncodedSdf::NONE
        },
        Sdf::CircleStroke(stroke) => EncodedSdf {
            kind: CUBE_SDF_CIRCLE_STROKE,
            coords: [
                stroke.circle.center.x as f32,
                stroke.circle.center.y as f32,
                stroke.circle.radius,
                0.0,
            ],
            stroke: [stroke.half_width; 4],
            ..EncodedSdf::NONE
        },
        Sdf::CircleShadow(shadow) => EncodedSdf {
            kind: CUBE_SDF_CIRCLE_SHADOW,
            coords: [
                shadow.circle.center.x as f32,
                shadow.circle.center.y as f32,
                shadow.circle.radius,
                0.0,
            ],
            shadow: [
                shadow.options.offset_x,
                shadow.options.offset_y,
                shadow.options.expand,
                shadow.options.intensity,
            ],
            ..EncodedSdf::NONE
        },
        Sdf::Arc(arc) => EncodedSdf {
            kind: CUBE_SDF_ARC,
            coords: [
                arc.center.x as f32,
                arc.center.y as f32,
                arc.radius,
                arc.width,
            ],
            radii: [arc.start_angle, arc.sweep_angle, arc.cap_value(), 0.0],
            ..EncodedSdf::NONE
        },
        Sdf::ArcShadow(shadow) => EncodedSdf {
            kind: CUBE_SDF_ARC_SHADOW,
            coords: [
                shadow.arc.center.x as f32,
                shadow.arc.center.y as f32,
                shadow.arc.radius,
                shadow.arc.width,
            ],
            radii: [
                shadow.arc.start_angle,
                shadow.arc.sweep_angle,
                shadow.arc.cap_value(),
                0.0,
            ],
            shadow: [
                shadow.options.offset_x,
                shadow.options.offset_y,
                shadow.options.expand,
                shadow.options.intensity,
            ],
            ..EncodedSdf::NONE
        },
        Sdf::CandleStick(candle) => EncodedSdf {
            kind: CUBE_SDF_CANDLESTICK,
            coords: [
                candle.center_x,
                candle.high_y,
                candle.low_y,
                candle.body_top_y,
            ],
            radii: [candle.body_bottom_y, candle.body_width as f32, 0.0, 0.0],
            ..EncodedSdf::NONE
        },
        Sdf::Line(line) => EncodedSdf {
            kind: CUBE_SDF_LINE,
            coords: [
                line.start.x as f32,
                line.start.y as f32,
                line.end.x as f32,
                line.end.y as f32,
            ],
            radii: [line.width, line.cap_value(), 0.0, 0.0],
            ..EncodedSdf::NONE
        },
        Sdf::LineShadow(shadow) => EncodedSdf {
            kind: CUBE_SDF_LINE_SHADOW,
            coords: [
                shadow.line.start.x as f32,
                shadow.line.start.y as f32,
                shadow.line.end.x as f32,
                shadow.line.end.y as f32,
            ],
            radii: [shadow.line.width, shadow.line.cap_value(), 0.0, 0.0],
            shadow: [
                shadow.options.offset_x,
                shadow.options.offset_y,
                shadow.options.expand,
                shadow.options.intensity,
            ],
            ..EncodedSdf::NONE
        },
    }
}
