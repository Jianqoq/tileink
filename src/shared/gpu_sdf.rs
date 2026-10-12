mod decode;
pub(crate) use decode::{decode_sdf, decode_sdf_shadow};

use crate::shared::{
    gpu_types::{
        GPU_SDF_ARC, GPU_SDF_ARC_SHADOW, GPU_SDF_CALLOUT, GPU_SDF_CALLOUT_SHADOW,
        GPU_SDF_CALLOUT_STROKE, GPU_SDF_CANDLESTICK, GPU_SDF_CHECKERBOARD, GPU_SDF_CIRCLE,
        GPU_SDF_CIRCLE_SHADOW, GPU_SDF_CIRCLE_STROKE, GPU_SDF_DASH_LINE, GPU_SDF_LINE,
        GPU_SDF_LINE_SHADOW, GPU_SDF_NONE, GPU_SDF_RECT, GPU_SDF_RECT_SHADOW, GPU_SDF_RECT_STROKE,
        GPU_SDF_STAR, GPU_SDF_STAR_STROKE, GPU_SDF_TRIANGLE,
    },
    sdf::{
        Sdf, SdfShadow,
        arc::{ArcShadow, Rc},
        callout::{Callout, CalloutShadow, CalloutSide, CalloutStroke, CalloutTail},
        candlestick::CandleStick,
        checkerboard::Checkerboard,
        circle::{Circle, CircleShadow, CircleStroke},
        line::{DashLine, Line, LineCap, LineShadow},
        rect::{Radius, Rect, RectShadow, RectStroke, StrokeWidths},
        shadow::ShadowOptions,
        star::{Star, StarStroke},
    },
};
use peniko::kurbo::Point;

pub(crate) const ENCODED_SDF_WORDS: usize = crate::shared::gpu_constants::SDF_RECORD_WORDS as usize;

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
        kind: GPU_SDF_NONE,
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
                kind: GPU_SDF_RECT,
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
                kind: GPU_SDF_RECT_STROKE,
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
        Sdf::Circle(circle) => EncodedSdf {
            kind: GPU_SDF_CIRCLE,
            coords: [
                circle.center.x as f32,
                circle.center.y as f32,
                circle.radius,
                0.0,
            ],
            ..EncodedSdf::NONE
        },
        Sdf::CircleStroke(stroke) => EncodedSdf {
            kind: GPU_SDF_CIRCLE_STROKE,
            coords: [
                stroke.circle.center.x as f32,
                stroke.circle.center.y as f32,
                stroke.circle.radius,
                0.0,
            ],
            stroke: [stroke.half_width; 4],
            ..EncodedSdf::NONE
        },
        Sdf::Rc(arc) => EncodedSdf {
            kind: GPU_SDF_ARC,
            coords: [
                arc.center.x as f32,
                arc.center.y as f32,
                arc.radius,
                arc.width,
            ],
            radii: [arc.start_angle, arc.sweep_angle, arc.cap_value(), 0.0],
            ..EncodedSdf::NONE
        },
        Sdf::CandleStick(candle) => EncodedSdf {
            kind: GPU_SDF_CANDLESTICK,
            coords: [
                candle.center_x,
                candle.high_y,
                candle.low_y,
                candle.body_top_y,
            ],
            radii: [
                candle.body_bottom_y,
                candle.body_width as f32,
                candle.wick_width as f32,
                0.0,
            ],
            ..EncodedSdf::NONE
        },
        Sdf::Line(line) => EncodedSdf {
            kind: GPU_SDF_LINE,
            coords: [
                line.start.x as f32,
                line.start.y as f32,
                line.end.x as f32,
                line.end.y as f32,
            ],
            radii: [line.width, line.cap_value(), 0.0, 0.0],
            ..EncodedSdf::NONE
        },
        Sdf::DashLine(line) => EncodedSdf {
            kind: GPU_SDF_DASH_LINE,
            coords: [
                line.line.start.x as f32,
                line.line.start.y as f32,
                line.line.end.x as f32,
                line.line.end.y as f32,
            ],
            radii: [
                line.line.width,
                line.line.cap_value(),
                line.dash_length,
                line.gap_length,
            ],
            stroke: [line.dash_offset, 0.0, 0.0, 0.0],
            ..EncodedSdf::NONE
        },
        Sdf::Triangle(triangle) => EncodedSdf {
            kind: GPU_SDF_TRIANGLE,
            coords: [
                triangle.a.x as f32,
                triangle.a.y as f32,
                triangle.b.x as f32,
                triangle.b.y as f32,
            ],
            radii: [
                triangle.c.x as f32,
                triangle.c.y as f32,
                triangle.corner_radius,
                0.0,
            ],
            ..EncodedSdf::NONE
        },
        Sdf::Checkerboard(checkerboard) => {
            let (x0, y0, x1, y1) = checkerboard.axis_bounds();
            EncodedSdf {
                kind: GPU_SDF_CHECKERBOARD,
                coords: [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
                radii: [checkerboard.cell_size, 0.0, 0.0, 0.0],
                ..EncodedSdf::NONE
            }
        }
        Sdf::Star(star) => encoded_star(star, GPU_SDF_STAR, 0.0),
        Sdf::StarStroke(stroke) => {
            encoded_star(stroke.star, GPU_SDF_STAR_STROKE, stroke.half_width)
        }
        Sdf::Callout(callout) => encoded_callout(callout, GPU_SDF_CALLOUT, 0.0),
        Sdf::CalloutStroke(stroke) => {
            encoded_callout(stroke.callout, GPU_SDF_CALLOUT_STROKE, stroke.half_width)
        }
    }
}

pub(crate) const fn encode_sdf_shadow(sdf_shadow: SdfShadow) -> EncodedSdf {
    match sdf_shadow {
        SdfShadow::Rect(shadow) => {
            let (x0, y0, x1, y1) = shadow.rect.axis_bounds();
            EncodedSdf {
                kind: GPU_SDF_RECT_SHADOW,
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
        SdfShadow::Circle(shadow) => EncodedSdf {
            kind: GPU_SDF_CIRCLE_SHADOW,
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
        SdfShadow::Rc(shadow) => EncodedSdf {
            kind: GPU_SDF_ARC_SHADOW,
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
        SdfShadow::Line(shadow) => EncodedSdf {
            kind: GPU_SDF_LINE_SHADOW,
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
        SdfShadow::Callout(shadow) => {
            let mut encoded = encoded_callout(shadow.callout, GPU_SDF_CALLOUT_SHADOW, 0.0);
            encoded.shadow = [
                shadow.options.offset_x,
                shadow.options.offset_y,
                shadow.options.expand,
                shadow.options.intensity,
            ];
            encoded
        }
    }
}

pub(crate) fn push_encoded_sdf(blob: &mut Vec<u32>, sdf: Sdf) -> (u32, u32) {
    push_encoded(blob, encode_sdf(sdf))
}

pub(crate) fn push_encoded_sdf_shadow(blob: &mut Vec<u32>, sdf_shadow: SdfShadow) -> (u32, u32) {
    push_encoded(blob, encode_sdf_shadow(sdf_shadow))
}

pub(crate) fn encoded_sdf(blob: &[u32], offset: u32, len: u32) -> Option<EncodedSdf> {
    let words = encoded_words(blob, offset, len)?;
    // A validated record contains exactly four four-float groups. Decode their
    // fixed layout directly, retaining the genuine truncated-record check above.
    let [coords, radii, stroke, shadow] = std::array::from_fn(|group| {
        std::array::from_fn(|component| f32::from_bits(words[1 + group * 4 + component]))
    });
    Some(EncodedSdf {
        kind: words[0],
        coords,
        radii,
        stroke,
        shadow,
    })
}

fn push_encoded(blob: &mut Vec<u32>, sdf: EncodedSdf) -> (u32, u32) {
    let offset = blob.len() as u32;
    blob.push(sdf.kind);
    for value in sdf
        .coords
        .into_iter()
        .chain(sdf.radii)
        .chain(sdf.stroke)
        .chain(sdf.shadow)
    {
        blob.push(value.to_bits());
    }
    (offset, ENCODED_SDF_WORDS as u32)
}

fn encoded_words(blob: &[u32], offset: u32, len: u32) -> Option<&[u32]> {
    let start = offset as usize;
    let end = start.checked_add(len as usize)?;
    let words = blob.get(start..end)?;
    (words.len() == ENCODED_SDF_WORDS).then_some(words)
}

const fn encoded_star(star: Star, kind: u32, half_width: f32) -> EncodedSdf {
    EncodedSdf {
        kind,
        coords: [
            star.center.x as f32,
            star.center.y as f32,
            star.outer_radius,
            star.inner_radius,
        ],
        radii: [star.corner_radius, star.rotation_radians, 0.0, 0.0],
        stroke: [half_width, 0.0, 0.0, 0.0],
        shadow: [0.0; 4],
    }
}

const fn encoded_callout(callout: Callout, kind: u32, half_width: f32) -> EncodedSdf {
    let (x0, y0, x1, y1) = callout.axis_bounds();
    EncodedSdf {
        kind,
        coords: [x0 as f32, y0 as f32, x1 as f32, y1 as f32],
        radii: [
            callout.body_radius,
            callout.tail.offset,
            callout.tail.width,
            callout.tail.length,
        ],
        stroke: [
            callout.tail.radius,
            callout.tail.side.value(),
            half_width,
            if callout.tail.visible { 1.0 } else { 0.0 },
        ],
        shadow: [0.0; 4],
    }
}

#[cfg(test)]
mod tests;
