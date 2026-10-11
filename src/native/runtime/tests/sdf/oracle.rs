//! f64 geometry for uniform rounded rectangles and circles is independent of
//! the native encoder and shader instruction order.
use super::*;

fn coverage(shape: Sdf, request: Request) -> u32 {
    let a = request.inverse;
    let [px, py] = request.point.map(f64::from);
    let x = f64::from(a.a) * px + f64::from(a.c) * py + f64::from(a.e);
    let y = f64::from(a.b) * px + f64::from(a.d) * py + f64::from(a.f);
    // Expected geometry comes from authored primitives, independently of the
    // production encoder and GPU blob interpretation.
    let (distance, nx, ny) = match shape {
        Sdf::Circle(circle) => {
            let (dx, dy) = (x - circle.center.x, y - circle.center.y);
            let length = dx.hypot(dy);
            let (nx, ny) = if length == 0.0 {
                (1.0, 0.0)
            } else {
                (dx / length, dy / length)
            };
            (length - f64::from(circle.radius), nx, ny)
        }
        Sdf::Rect(rect) => {
            let (x0, x1) = (rect.start.x.min(rect.end.x), rect.start.x.max(rect.end.x));
            let (y0, y1) = (rect.start.y.min(rect.end.y), rect.start.y.max(rect.end.y));
            let radius = f64::from(rect.radius.top_left).clamp(0.0, ((x1 - x0).min(y1 - y0)) * 0.5);
            let nearest = (
                x.clamp(x0 + radius, x1 - radius),
                y.clamp(y0 + radius, y1 - radius),
            );
            let (dx, dy) = (x - nearest.0, y - nearest.1);
            let length = dx.hypot(dy);
            if length > 0.0 {
                (length - radius, dx / length, dy / length)
            } else {
                let (dx, dy) = (
                    (x - (x0 + x1) * 0.5).abs() - (x1 - x0) * 0.5,
                    (y - (y0 + y1) * 0.5).abs() - (y1 - y0) * 0.5,
                );
                if dx > dy {
                    (dx, if x >= (x0 + x1) * 0.5 { 1.0 } else { -1.0 }, 0.0)
                } else {
                    (dy, 0.0, if y >= (y0 + y1) * 0.5 { 1.0 } else { -1.0 })
                }
            }
        }
        _ => unreachable!("analytic oracle accepts rectangles and circles"),
    };
    let gradient = (f64::from(a.a) * nx + f64::from(a.b) * ny)
        .hypot(f64::from(a.c) * nx + f64::from(a.d) * ny);
    ((0.5 - distance / gradient).clamp(0.0, 1.0) * 255.0).round() as u32
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_identity_rect_circle_match_exact_independent_oracles() -> Result<()> {
    let context = context()?;
    let shapes = [
        Sdf::Rect(SdfRect {
            start: Point::new(2.0, 2.0),
            end: Point::new(10.0, 8.0),
            radius: Radius::ZERO,
        }),
        Sdf::Circle(SdfCircle {
            center: Point::new(6.0, 6.0),
            radius: 4.0,
        }),
    ];
    let records = shapes.map(encode_sdf);
    let mut requests = Vec::new();
    for record in 0..2 {
        for phase in [0.0, 0.5] {
            for y in 0..12 {
                for x in 0..14 {
                    requests.push(Request {
                        record,
                        point: [x as f32 + phase, y as f32 + phase],
                        inverse: GpuAffine::IDENTITY,
                    });
                }
            }
        }
    }
    let expected: Vec<_> = requests
        .iter()
        .map(|&r| coverage(shapes[r.record as usize], r))
        .collect();
    assert_eq!(sample(&context, &records, &requests)?, expected);
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_rect_circle_match_analytic_coverage_under_affines() -> Result<()> {
    let context = context()?;
    let mut shapes = Vec::new();
    for radius in [0.0, 2.0, 20.0] {
        for (start, end) in [((2.0, 3.0), (12.0, 11.0)), ((12.0, 11.0), (2.0, 3.0))] {
            shapes.push(Sdf::Rect(SdfRect {
                start: start.into(),
                end: end.into(),
                radius: Radius::all(radius),
            }));
        }
        shapes.push(Sdf::Circle(SdfCircle {
            center: Point::new(8.0, 8.0),
            radius,
        }));
    }
    let records: Vec<_> = shapes.iter().copied().map(encode_sdf).collect();
    let mut requests = Vec::new();
    let points = points();
    for record in 0..records.len() {
        for inverse in inverses() {
            for &point in &points {
                requests.push(Request {
                    record: record as u32,
                    point,
                    inverse,
                });
            }
        }
    }
    for (&actual, &request) in sample(&context, &records, &requests)?.iter().zip(&requests) {
        assert_close(
            actual,
            coverage(shapes[request.record as usize], request),
            request,
        );
    }
    context.check_validation()?;
    Ok(())
}
