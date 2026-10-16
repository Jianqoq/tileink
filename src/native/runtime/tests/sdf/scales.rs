//! Magnified analytic primitives retain coverage independently of their authored coordinate scale.
use super::*;

const SCALES: [f32; 5] = [1.0e-20, 1.0e-10, 1.0e-6, 1.0, 1.0e20];

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_lines_and_dash_gaps_preserve_coverage_at_every_scale() -> Result<()> {
    let context = context()?;
    for size in SCALES {
        let line = SdfLine::new(
            Point::ZERO,
            Point::new(f64::from(size), 0.0),
            size * 0.4,
            SdfLineCap::Butt,
        );
        let dash = SdfDashLine::new(
            Point::ZERO,
            Point::new(f64::from(size) * 4.0, 0.0),
            size * 0.1,
            SdfLineCap::Butt,
            size * 0.25,
            size * 0.25,
        );
        let inverse = GpuAffine::from_logical(
            Affine::scale_non_uniform(f64::from(size), f64::from(size) * 2.0),
            1.0,
        );
        let requests = [
            Request {
                record: 0,
                point: [0.5, 0.125],
                inverse,
            },
            Request {
                record: 0,
                point: [0.5, 0.05],
                inverse,
            },
            Request {
                record: 1,
                point: [0.375, 0.0],
                inverse,
            },
            Request {
                record: 2,
                point: [0.5, 0.125],
                inverse,
            },
        ];
        let actual = sample(
            &context,
            &[
                encode_sdf(Sdf::Line(line)),
                encode_sdf(Sdf::DashLine(dash)),
                encode_sdf(Sdf::Line(SdfLine::new(
                    Point::ZERO,
                    Point::ZERO,
                    size * 0.4,
                    SdfLineCap::Butt,
                ))),
            ],
            &requests,
        )?;
        for (actual, expected) in actual.into_iter().zip([0.475_f32, 0.55, 0.375, 0.0]) {
            assert_close(actual, (expected * 255.0).round() as u32, size);
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_arcs_preserve_positive_geometry_at_every_scale() -> Result<()> {
    let context = context()?;
    for size in SCALES {
        for sweep in [-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2] {
            let arc = SdfArc::new(Point::ZERO, size, 0.0, sweep, size * 0.4, SdfLineCap::Butt);
            let inverse = GpuAffine::from_logical(Affine::scale(f64::from(size)), 1.0);
            let wide = SdfArc::new(Point::ZERO, size, 0.0, sweep, size * 4.0, SdfLineCap::Butt);
            let almost_full = SdfArc::new(
                Point::ZERO,
                size,
                0.0,
                sweep.signum() * (std::f32::consts::TAU - 0.00005),
                size * 0.2,
                SdfLineCap::Butt,
            );
            let requests = [
                Request {
                    record: 0,
                    point: [0.8, 0.8 * sweep.signum()],
                    inverse,
                },
                Request {
                    record: 1,
                    point: [0.0, 0.0],
                    inverse,
                },
                Request {
                    record: 2,
                    point: [1.0, -2.5 * sweep.signum()],
                    inverse: GpuAffine::from_logical(
                        Affine::scale_non_uniform(f64::from(size), f64::from(size) * 1.0e-5),
                        1.0,
                    ),
                },
            ];
            let actual = sample(
                &context,
                &[arc, wide, almost_full].map(|a| encode_sdf(Sdf::Rc(a))),
                &requests,
            )?;
            let expected = ((0.5_f64 - (0.8_f64.hypot(0.8) - 1.0 - 0.2)) * 255.0).round() as u32;
            assert_close(actual[0], expected, (size, sweep));
            assert_close(
                actual[1],
                128,
                (size, "wide butt arc center is a wedge boundary"),
            );
            assert_eq!(
                actual[2], 0,
                "near-full sweep must retain its magnified gap: {size}, {sweep}"
            );
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_round_boundaries_match_the_f64_oracle_at_every_scale() -> Result<()> {
    let context = context()?;
    for size in SCALES {
        let s = f64::from(size);
        let shapes = [
            Sdf::Rect(SdfRect {
                start: Point::new(-s, -s),
                end: Point::new(s, s),
                radius: Radius::all(size * 0.25),
            }),
            Sdf::Circle(SdfCircle {
                center: Point::ZERO,
                radius: size,
            }),
        ];
        let inverse = GpuAffine::from_logical(Affine::scale_non_uniform(s, s * 2.0), 1.0);
        let requests = [
            Request {
                record: 0,
                point: [0.8, 0.4],
                inverse,
            },
            Request {
                record: 1,
                point: [1.1, 0.125],
                inverse,
            },
        ];
        let actual = sample(&context, &shapes.map(encode_sdf), &requests)?;
        for (index, actual) in actual.into_iter().enumerate() {
            assert_close(
                actual,
                oracle::coverage(shapes[index], requests[index]),
                (size, index),
            );
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_polygon_unions_preserve_authored_coordinate_scale() -> Result<()> {
    let context = context()?;
    let at_scale = |size: f32| -> Result<Vec<u32>> {
        let s = f64::from(size);
        let callout = SdfCallout::new(
            peniko::kurbo::Rect::new(-4.0 * s, 0.0, 4.0 * s, 4.0 * s),
            0.0,
            SdfCalloutTail::new(
                SdfCalloutSide::Top,
                4.0 * size,
                2.0 * size,
                2.0 * size,
                0.1 * size,
            ),
        );
        let inverse = GpuAffine::from_logical(Affine::scale(s), 1.0);
        let mut requests = [[0.25, -0.75], [1.0, -0.75], [-1.1, -1.2]]
            .map(|point| Request {
                record: 0,
                point,
                inverse,
            })
            .to_vec();
        requests.extend(
            [[0.7, 0.1], [0.1, 0.1], [-0.1, 0.1], [0.0, 0.3]].map(|point| Request {
                record: 1,
                point,
                inverse: GpuAffine::from_logical(Affine::scale(s * 5.0), 1.0),
            }),
        );
        let star = SdfStar::new(Point::ZERO, 2.0 * size, size, 0.0, 0.0);
        sample(
            &context,
            &[
                encode_sdf(Sdf::Callout(callout)),
                encode_sdf(Sdf::Star(star)),
            ],
            &requests,
        )
    };
    let reference = at_scale(1.0)?;
    for size in SCALES.into_iter().filter(|size| *size != 1.0) {
        for (index, (actual, expected)) in at_scale(size)?.into_iter().zip(&reference).enumerate() {
            assert_close(actual, *expected, (size, index));
        }
    }
    context.check_validation()?;
    Ok(())
}
