use crate::shared::{
    gpu_types::{
        GPU_SDF_ARC, GPU_SDF_ARC_SHADOW, GPU_SDF_CANDLESTICK, GPU_SDF_CIRCLE,
        GPU_SDF_CIRCLE_SHADOW, GPU_SDF_CIRCLE_STROKE, GPU_SDF_DASH_LINE, GPU_SDF_LINE,
        GPU_SDF_LINE_SHADOW, GPU_SDF_NONE, GPU_SDF_RECT, GPU_SDF_RECT_SHADOW, GPU_SDF_RECT_STROKE,
        GPU_SDF_TRIANGLE,
    },
    sdf::{
        Sdf, SdfShadow,
        arc::{ArcShadow, Rc},
        candlestick::CandleStick,
        circle::{Circle, CircleShadow, CircleStroke},
        line::{DashLine, Line, LineCap, LineShadow},
        rect::{Radius, Rect, RectShadow, RectStroke, StrokeWidths},
        shadow::ShadowOptions,
    },
};
use peniko::kurbo::Point;

pub(crate) const ENCODED_SDF_WORDS: usize = 17;

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
    }
}

pub(crate) fn encode_sdf_shadow(sdf_shadow: SdfShadow) -> EncodedSdf {
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
    let mut values = [0.0; 16];
    for (dst, src) in values.iter_mut().zip(&words[1..]) {
        *dst = f32::from_bits(*src);
    }
    Some(EncodedSdf {
        kind: words[0],
        coords: values[0..4].try_into().ok()?,
        radii: values[4..8].try_into().ok()?,
        stroke: values[8..12].try_into().ok()?,
        shadow: values[12..16].try_into().ok()?,
    })
}

pub(crate) fn decode_sdf(blob: &[u32], offset: u32, len: u32) -> Option<Sdf> {
    let sdf = encoded_sdf(blob, offset, len)?;
    match sdf.kind {
        GPU_SDF_RECT => Some(Sdf::Rect(rect_from_encoded(sdf))),
        GPU_SDF_RECT_STROKE => Some(Sdf::RectStroke(RectStroke {
            rect: rect_from_encoded(sdf),
            widths: StrokeWidths {
                top: sdf.stroke[0] * 2.0,
                right: sdf.stroke[1] * 2.0,
                bottom: sdf.stroke[2] * 2.0,
                left: sdf.stroke[3] * 2.0,
            },
        })),
        GPU_SDF_CIRCLE => Some(Sdf::Circle(circle_from_encoded(sdf))),
        GPU_SDF_CIRCLE_STROKE => Some(Sdf::CircleStroke(CircleStroke {
            circle: circle_from_encoded(sdf),
            half_width: sdf.stroke[0],
        })),
        GPU_SDF_ARC => Some(Sdf::Rc(arc_from_encoded(sdf))),
        GPU_SDF_CANDLESTICK => Some(Sdf::CandleStick(CandleStick {
            center_x: sdf.coords[0],
            high_y: sdf.coords[1],
            low_y: sdf.coords[2],
            body_top_y: sdf.coords[3],
            body_bottom_y: sdf.radii[0],
            body_width: sdf.radii[1] as u32,
            wick_width: sdf.radii[2] as u32,
        })),
        GPU_SDF_LINE => Some(Sdf::Line(line_from_encoded(sdf))),
        GPU_SDF_DASH_LINE => Some(Sdf::DashLine(DashLine {
            line: line_from_encoded(sdf),
            dash_length: sdf.radii[2],
            gap_length: sdf.radii[3],
            dash_offset: sdf.stroke[0],
        })),
        GPU_SDF_TRIANGLE => Some(Sdf::Triangle(triangle_from_encoded(sdf))),
        _ => None,
    }
}

pub(crate) fn decode_sdf_shadow(blob: &[u32], offset: u32, len: u32) -> Option<SdfShadow> {
    let sdf = encoded_sdf(blob, offset, len)?;
    match sdf.kind {
        GPU_SDF_RECT_SHADOW => Some(SdfShadow::Rect(RectShadow {
            rect: rect_from_encoded(sdf),
            options: shadow_options_from_encoded(sdf),
        })),
        GPU_SDF_CIRCLE_SHADOW => Some(SdfShadow::Circle(CircleShadow {
            circle: circle_from_encoded(sdf),
            options: shadow_options_from_encoded(sdf),
        })),
        GPU_SDF_ARC_SHADOW => Some(SdfShadow::Rc(ArcShadow {
            arc: arc_from_encoded(sdf),
            options: shadow_options_from_encoded(sdf),
        })),
        GPU_SDF_LINE_SHADOW => Some(SdfShadow::Line(LineShadow {
            line: line_from_encoded(sdf),
            options: shadow_options_from_encoded(sdf),
        })),
        _ => None,
    }
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

fn rect_from_encoded(sdf: EncodedSdf) -> Rect {
    Rect {
        start: Point::new(f64::from(sdf.coords[0]), f64::from(sdf.coords[1])),
        end: Point::new(f64::from(sdf.coords[2]), f64::from(sdf.coords[3])),
        radius: Radius {
            top_left: sdf.radii[0],
            top_right: sdf.radii[1],
            bottom_left: sdf.radii[2],
            bottom_right: sdf.radii[3],
        },
    }
}

fn circle_from_encoded(sdf: EncodedSdf) -> Circle {
    Circle {
        center: Point::new(f64::from(sdf.coords[0]), f64::from(sdf.coords[1])),
        radius: sdf.coords[2],
    }
}

fn triangle_from_encoded(sdf: EncodedSdf) -> crate::shared::sdf::triangle::Triangle {
    crate::shared::sdf::triangle::Triangle {
        a: Point::new(f64::from(sdf.coords[0]), f64::from(sdf.coords[1])),
        b: Point::new(f64::from(sdf.coords[2]), f64::from(sdf.coords[3])),
        c: Point::new(f64::from(sdf.radii[0]), f64::from(sdf.radii[1])),
        corner_radius: sdf.radii[2],
    }
}

fn line_from_encoded(sdf: EncodedSdf) -> Line {
    Line {
        start: Point::new(f64::from(sdf.coords[0]), f64::from(sdf.coords[1])),
        end: Point::new(f64::from(sdf.coords[2]), f64::from(sdf.coords[3])),
        width: sdf.radii[0],
        cap: line_cap_from_value(sdf.radii[1]),
    }
}

fn arc_from_encoded(sdf: EncodedSdf) -> Rc {
    Rc {
        center: Point::new(f64::from(sdf.coords[0]), f64::from(sdf.coords[1])),
        radius: sdf.coords[2],
        width: sdf.coords[3],
        start_angle: sdf.radii[0],
        sweep_angle: sdf.radii[1],
        cap: line_cap_from_value(sdf.radii[2]),
    }
}

fn shadow_options_from_encoded(sdf: EncodedSdf) -> ShadowOptions {
    ShadowOptions {
        offset_x: sdf.shadow[0],
        offset_y: sdf.shadow[1],
        expand: sdf.shadow[2],
        intensity: sdf.shadow[3],
    }
}

fn line_cap_from_value(value: f32) -> LineCap {
    match value as u32 {
        value if value == LineCap::Square as u32 => LineCap::Square,
        value if value == LineCap::Round as u32 => LineCap::Round,
        _ => LineCap::Butt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_sdf_blob_round_trips_sdf_and_shadow_records() {
        let sdfs = [
            Sdf::Rect(Rect {
                start: Point::new(2.0, 3.0),
                end: Point::new(18.0, 19.0),
                radius: Radius::all(4.0),
            }),
            Sdf::RectStroke(RectStroke {
                rect: Rect {
                    start: Point::new(4.0, 5.0),
                    end: Point::new(40.0, 41.0),
                    radius: Radius {
                        top_left: 1.0,
                        top_right: 2.0,
                        bottom_right: 3.0,
                        bottom_left: 4.0,
                    },
                },
                widths: StrokeWidths {
                    top: 2.0,
                    right: 4.0,
                    bottom: 6.0,
                    left: 8.0,
                },
            }),
            Sdf::DashLine(DashLine::with_offset(
                Point::new(8.0, 16.5),
                Point::new(32.0, 16.5),
                1.0,
                LineCap::Square,
                4.0,
                3.0,
                1.5,
            )),
            Sdf::Triangle(crate::shared::sdf::triangle::Triangle::new(
                Point::new(4.0, 5.0),
                Point::new(20.0, 12.0),
                Point::new(7.0, 30.0),
                2.5,
            )),
        ];
        let shadows = [
            SdfShadow::Circle(CircleShadow {
                circle: Circle {
                    center: Point::new(20.0, 21.0),
                    radius: 8.0,
                },
                options: ShadowOptions::new(2.0, 3.0, 4.0, 0.5),
            }),
            SdfShadow::Rc(ArcShadow {
                arc: Rc::new(
                    Point::new(32.0, 32.0),
                    10.0,
                    0.0,
                    std::f32::consts::FRAC_PI_2,
                    3.0,
                    LineCap::Round,
                ),
                options: ShadowOptions::new(-1.0, 2.0, 5.0, 0.75),
            }),
        ];
        let mut sdf_blob = Vec::new();
        for sdf in sdfs {
            let (offset, len) = push_encoded_sdf(&mut sdf_blob, sdf);
            let decoded = decode_sdf(&sdf_blob, offset, len).expect("decode sdf");
            assert_eq!(encode_sdf(decoded), encode_sdf(sdf));
        }

        let mut shadow_blob = Vec::new();
        for shadow in shadows {
            let (offset, len) = push_encoded_sdf_shadow(&mut shadow_blob, shadow);
            let decoded = decode_sdf_shadow(&shadow_blob, offset, len).expect("decode sdf shadow");
            assert_eq!(encode_sdf_shadow(decoded), encode_sdf_shadow(shadow));
        }
    }
}
