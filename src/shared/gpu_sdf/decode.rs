//! Reconstruct CPU shapes only from valid encoded descriptors. Discrete fields
//! must be exact encoder values; truncation or defaults would hide corrupt data.
use super::*;

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
        GPU_SDF_ARC => Some(Sdf::Rc(arc_from_encoded(sdf)?)),
        GPU_SDF_CANDLESTICK => Some(Sdf::CandleStick(CandleStick {
            center_x: sdf.coords[0],
            high_y: sdf.coords[1],
            low_y: sdf.coords[2],
            body_top_y: sdf.coords[3],
            body_bottom_y: sdf.radii[0],
            body_width: sdf.radii[1] as u32,
            wick_width: sdf.radii[2] as u32,
        })),
        GPU_SDF_LINE => Some(Sdf::Line(line_from_encoded(sdf)?)),
        GPU_SDF_DASH_LINE => Some(Sdf::DashLine(DashLine {
            line: line_from_encoded(sdf)?,
            dash_length: sdf.radii[2],
            gap_length: sdf.radii[3],
            dash_offset: sdf.stroke[0],
        })),
        GPU_SDF_TRIANGLE => Some(Sdf::Triangle(triangle_from_encoded(sdf))),
        GPU_SDF_CHECKERBOARD => Some(Sdf::Checkerboard(Checkerboard {
            start: Point::new(f64::from(sdf.coords[0]), f64::from(sdf.coords[1])),
            end: Point::new(f64::from(sdf.coords[2]), f64::from(sdf.coords[3])),
            cell_size: sdf.radii[0],
        })),
        GPU_SDF_STAR => Some(Sdf::Star(star_from_encoded(sdf))),
        GPU_SDF_STAR_STROKE => Some(Sdf::StarStroke(StarStroke {
            star: star_from_encoded(sdf),
            half_width: sdf.stroke[0],
        })),
        GPU_SDF_CALLOUT => Some(Sdf::Callout(callout_from_encoded(sdf)?)),
        GPU_SDF_CALLOUT_STROKE => Some(Sdf::CalloutStroke(CalloutStroke {
            callout: callout_from_encoded(sdf)?,
            half_width: sdf.stroke[2],
        })),
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
            arc: arc_from_encoded(sdf)?,
            options: shadow_options_from_encoded(sdf),
        })),
        GPU_SDF_LINE_SHADOW => Some(SdfShadow::Line(LineShadow {
            line: line_from_encoded(sdf)?,
            options: shadow_options_from_encoded(sdf),
        })),
        GPU_SDF_CALLOUT_SHADOW => Some(SdfShadow::Callout(CalloutShadow {
            callout: callout_from_encoded(sdf)?,
            options: shadow_options_from_encoded(sdf),
        })),
        _ => None,
    }
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

fn callout_from_encoded(sdf: EncodedSdf) -> Option<Callout> {
    Some(Callout {
        start: Point::new(f64::from(sdf.coords[0]), f64::from(sdf.coords[1])),
        end: Point::new(f64::from(sdf.coords[2]), f64::from(sdf.coords[3])),
        body_radius: sdf.radii[0],
        tail: CalloutTail {
            visible: match sdf.stroke[3] {
                0.0 => false,
                1.0 => true,
                _ => return None,
            },
            side: CalloutSide::from_value(sdf.stroke[1])?,
            offset: sdf.radii[1],
            width: sdf.radii[2],
            length: sdf.radii[3],
            radius: sdf.stroke[0],
        },
    })
}

fn star_from_encoded(sdf: EncodedSdf) -> Star {
    Star {
        center: Point::new(f64::from(sdf.coords[0]), f64::from(sdf.coords[1])),
        outer_radius: sdf.coords[2],
        inner_radius: sdf.coords[3],
        corner_radius: sdf.radii[0],
        rotation_radians: sdf.radii[1],
    }
}

fn line_from_encoded(sdf: EncodedSdf) -> Option<Line> {
    Some(Line {
        start: Point::new(f64::from(sdf.coords[0]), f64::from(sdf.coords[1])),
        end: Point::new(f64::from(sdf.coords[2]), f64::from(sdf.coords[3])),
        width: sdf.radii[0],
        cap: line_cap_from_value(sdf.radii[1])?,
    })
}

fn arc_from_encoded(sdf: EncodedSdf) -> Option<Rc> {
    Some(Rc {
        center: Point::new(f64::from(sdf.coords[0]), f64::from(sdf.coords[1])),
        radius: sdf.coords[2],
        width: sdf.coords[3],
        start_angle: sdf.radii[0],
        sweep_angle: sdf.radii[1],
        cap: line_cap_from_value(sdf.radii[2])?,
    })
}

const fn shadow_options_from_encoded(sdf: EncodedSdf) -> ShadowOptions {
    ShadowOptions {
        offset_x: sdf.shadow[0],
        offset_y: sdf.shadow[1],
        expand: sdf.shadow[2],
        intensity: sdf.shadow[3],
    }
}

fn line_cap_from_value(value: f32) -> Option<LineCap> {
    match value {
        0.0 => Some(LineCap::Butt),
        1.0 => Some(LineCap::Square),
        2.0 => Some(LineCap::Round),
        _ => None,
    }
}
