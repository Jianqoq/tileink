use super::*;

fn equal(context: &NativeContext, a: EncodedSdf, b: EncodedSdf) -> Result<()> {
    let mut requests = Vec::new();
    let points = points();
    for inverse in inverses() {
        for &point in &points {
            requests.extend([
                Request {
                    record: 0,
                    point,
                    inverse,
                },
                Request {
                    record: 1,
                    point,
                    inverse,
                },
            ]);
        }
    }
    for (point, pair) in sample(context, &[a, b], &requests)?
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
    {
        assert_close(pair[0], pair[1], (a.kind, b.kind, requests[point * 2]));
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_degenerate_line_caps_and_full_arcs_have_geometric_equivalents() -> Result<()> {
    let context = context()?;
    let center = Point::new(8.0, 8.0);
    for (cap, equivalent) in [
        (SdfLineCap::Butt, EncodedSdf::NONE),
        (
            SdfLineCap::Round,
            encode_sdf(Sdf::Circle(SdfCircle {
                center,
                radius: 2.0,
            })),
        ),
        (
            SdfLineCap::Square,
            encode_sdf(Sdf::Rect(SdfRect {
                start: Point::new(6.0, 6.0),
                end: Point::new(10.0, 10.0),
                radius: Radius::ZERO,
            })),
        ),
    ] {
        equal(
            &context,
            encode_sdf(Sdf::Line(SdfLine::new(center, center, 4.0, cap))),
            equivalent,
        )?;
        // The stroke spans the largest transformed pixel footprint, so the
        // inner/outer coverage ramps do not overlap.
        for sweep in [-std::f32::consts::TAU, std::f32::consts::TAU] {
            equal(
                &context,
                encode_sdf(Sdf::Rc(SdfArc::new(center, 5.0, 0.3, sweep, 2.0, cap))),
                encode_sdf(Sdf::CircleStroke(SdfCircleStroke {
                    circle: SdfCircle {
                        center,
                        radius: 5.0,
                    },
                    half_width: 1.0,
                })),
            )?;
        }
        equal(
            &context,
            encode_sdf(Sdf::Rc(SdfArc::new(center, 5.0, 0.3, 0.0, 2.0, cap))),
            EncodedSdf::NONE,
        )?;
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_hidden_callout_tails_preserve_body_stroke_and_shadow() -> Result<()> {
    let context = context()?;
    let bounds = peniko::kurbo::Rect::new(2.0, 3.0, 12.0, 11.0);
    let rect = SdfRect {
        start: Point::new(2.0, 3.0),
        end: Point::new(12.0, 11.0),
        radius: Radius::all(2.0),
    };
    for side in [
        SdfCalloutSide::Top,
        SdfCalloutSide::Right,
        SdfCalloutSide::Bottom,
        SdfCalloutSide::Left,
    ] {
        let callout = SdfCallout::new(
            bounds,
            2.0,
            SdfCalloutTail {
                side,
                ..SdfCalloutTail::hidden()
            },
        );
        equal(
            &context,
            encode_sdf(Sdf::Callout(callout)),
            encode_sdf(Sdf::Rect(rect)),
        )?;
        // A two-unit stroke spans every pixel footprint in these inverses.
        equal(
            &context,
            encode_sdf(Sdf::CalloutStroke(SdfCalloutStroke::new(callout, 2.0))),
            encode_sdf(Sdf::RectStroke(SdfRectStroke {
                rect,
                widths: StrokeWidths::all(2.0),
            })),
        )?;
        for expand in [0.0, 2.5] {
            let options = ShadowOptions::new(0.75, -0.5, expand, 0.7);
            equal(
                &context,
                encode_sdf_shadow(SdfShadow::Callout(SdfCalloutShadow { callout, options })),
                encode_sdf_shadow(SdfShadow::Rect(SdfRectShadow { rect, options })),
            )?;
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_dash_phase_is_periodic_for_each_line_cap() -> Result<()> {
    let context = context()?;
    for cap in [SdfLineCap::Butt, SdfLineCap::Square, SdfLineCap::Round] {
        let dash = SdfDashLine::with_offset(
            Point::new(2.0, 3.0),
            Point::new(12.0, 11.0),
            2.0,
            cap,
            3.0,
            2.0,
            1.25,
        );
        for periods in [-7.0, -1.0, 1.0, 9.0] {
            let shifted = SdfDashLine {
                dash_offset: dash.dash_offset + periods * (dash.dash_length + dash.gap_length),
                ..dash
            };
            equal(
                &context,
                encode_sdf(Sdf::DashLine(dash)),
                encode_sdf(Sdf::DashLine(shifted)),
            )?;
        }
    }
    context.check_validation()?;
    Ok(())
}
