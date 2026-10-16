use super::*;

pub(super) fn records() -> Vec<EncodedSdf> {
    let rect = SdfRect {
        start: Point::new(2.0, 3.0),
        end: Point::new(12.0, 11.0),
        radius: Radius::all(2.0),
    };
    let circle = SdfCircle {
        center: Point::new(8.0, 8.0),
        radius: 5.0,
    };
    let star = SdfStar::new(Point::new(8.0, 8.0), 6.0, 3.0, 0.5, 0.3);
    let options = ShadowOptions::new(0.75, -0.5, 2.5, 0.7);
    let mut shapes = vec![
        Sdf::Rect(rect),
        Sdf::RectStroke(SdfRectStroke {
            rect,
            widths: StrokeWidths {
                top: 2.0,
                right: 3.0,
                bottom: 1.0,
                left: 4.0,
            },
        }),
        Sdf::Circle(circle),
        Sdf::CircleStroke(SdfCircleStroke {
            circle,
            half_width: 1.25,
        }),
        Sdf::CandleStick(CandleStick::new(8.0, 2.0, 14.0, 5.0, 11.0, 5, 1)),
        Sdf::Triangle(SdfTriangle::new(
            Point::new(2.0, 3.0),
            Point::new(12.0, 5.0),
            Point::new(5.0, 13.0),
            0.75,
        )),
        Sdf::Checkerboard(SdfCheckerboard::new(
            peniko::kurbo::Rect::new(2.0, 3.0, 12.0, 11.0),
            3.0,
        )),
        Sdf::Star(star),
        Sdf::StarStroke(SdfStarStroke::new(star, 2.5)),
    ];
    let mut shadows = vec![
        SdfShadow::Rect(SdfRectShadow { rect, options }),
        SdfShadow::Circle(SdfCircleShadow { circle, options }),
    ];
    for cap in [SdfLineCap::Butt, SdfLineCap::Square, SdfLineCap::Round] {
        let line = SdfLine::new(Point::new(2.0, 3.0), Point::new(12.0, 11.0), 2.0, cap);
        let arc = SdfArc::new(Point::new(8.0, 8.0), 5.0, -0.75, 4.5, 2.5, cap);
        shapes.extend([
            Sdf::Line(line),
            Sdf::Rc(arc),
            Sdf::DashLine(SdfDashLine::with_offset(
                line.start, line.end, line.width, cap, 3.0, 2.0, -1.0,
            )),
        ]);
        shadows.extend([
            SdfShadow::Line(SdfLineShadow { line, options }),
            SdfShadow::Rc(SdfArcShadow { arc, options }),
        ]);
    }
    for side in [
        SdfCalloutSide::Top,
        SdfCalloutSide::Right,
        SdfCalloutSide::Bottom,
        SdfCalloutSide::Left,
    ] {
        for visible in [false, true] {
            let tail = SdfCalloutTail {
                visible,
                ..SdfCalloutTail::new(side, 5.0, 4.0, 3.0, 0.5)
            };
            let callout =
                SdfCallout::new(peniko::kurbo::Rect::new(2.0, 3.0, 12.0, 11.0), 2.0, tail);
            shapes.extend([
                Sdf::Callout(callout),
                Sdf::CalloutStroke(SdfCalloutStroke::new(callout, 2.5)),
            ]);
            shadows.push(SdfShadow::Callout(SdfCalloutShadow { callout, options }));
        }
    }
    shapes
        .into_iter()
        .map(encode_sdf)
        .chain(shadows.into_iter().map(encode_sdf_shadow))
        .collect()
}
