//! Independent boundary-distance oracle for CSS's four circular corner arcs.
use super::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

struct Case {
    size: [f64; 2],
    authored: [f32; 4],
    resolved: [f64; 4],
}

fn closest_on_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0] * d[0] + d[1] * d[1];
    let t = if length == 0. {
        0.
    } else {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length).clamp(0., 1.)
    };
    [a[0] + t * d[0], a[1] + t * d[1]]
}

// The oracle measures actual line/arc boundary distances; it does not select a corner by
// the point's midpoint quadrant or call the production SDF/normalization implementation.
fn coverage(case: &Case, request: Request) -> u32 {
    let a = request.inverse;
    let [px, py] = request.point.map(f64::from);
    let p = [
        f64::from(a.a) * px + f64::from(a.c) * py + f64::from(a.e),
        f64::from(a.b) * px + f64::from(a.d) * py + f64::from(a.f),
    ];
    let [w, h] = case.size;
    let [tl, tr, bl, br] = case.resolved;
    let mut candidates = Vec::with_capacity(16);
    for (from, to) in [
        ([tl, 0.], [w - tr, 0.]),
        ([w, tr], [w, h - br]),
        ([w - br, h], [bl, h]),
        ([0., h - bl], [0., tl]),
    ] {
        candidates.push(closest_on_segment(p, from, to));
    }
    let arcs = [
        ([tl, tl], tl, PI),
        ([w - tr, tr], tr, 3. * FRAC_PI_2),
        ([w - br, h - br], br, 0.),
        ([bl, h - bl], bl, FRAC_PI_2),
    ];
    let mut inside = p[0] >= 0. && p[0] <= w && p[1] >= 0. && p[1] <= h;
    for (index, (center, radius, start)) in arcs.into_iter().enumerate() {
        if radius == 0. {
            continue;
        }
        let delta = [p[0] - center[0], p[1] - center[1]];
        let region = match index {
            0 => delta[0] < 0. && delta[1] < 0.,
            1 => delta[0] > 0. && delta[1] < 0.,
            2 => delta[0] > 0. && delta[1] > 0.,
            _ => delta[0] < 0. && delta[1] > 0.,
        };
        inside &= !region || delta[0].hypot(delta[1]) <= radius;
        let angle = delta[1].atan2(delta[0]).rem_euclid(TAU);
        for angle in [start, start + FRAC_PI_2] {
            candidates.push([
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]);
        }
        if angle >= start && angle <= start + FRAC_PI_2 {
            candidates.push([
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]);
        }
    }
    let nearest = candidates
        .into_iter()
        .min_by(|a, b| {
            (p[0] - a[0])
                .hypot(p[1] - a[1])
                .total_cmp(&(p[0] - b[0]).hypot(p[1] - b[1]))
        })
        .unwrap();
    let length = (p[0] - nearest[0]).hypot(p[1] - nearest[1]);
    if length == 0. {
        return 128;
    }
    let sign = if inside { -1. } else { 1. };
    let normal = [
        (p[0] - nearest[0]) * sign / length,
        (p[1] - nearest[1]) * sign / length,
    ];
    let gradient = (f64::from(a.a) * normal[0] + f64::from(a.b) * normal[1])
        .hypot(f64::from(a.c) * normal[0] + f64::from(a.d) * normal[1]);
    ((0.5 - sign * length / gradient).clamp(0., 1.) * 255.).round() as u32
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_css_corner_arcs_match_independent_boundary_geometry_across_midpoints_and_affines()
-> Result<()> {
    let context = context()?;
    let mut cases: Vec<_> = (0..4)
        .map(|index| {
            let mut radii = [0.; 4];
            radii[index] = 6.;
            Case {
                size: [8., 8.],
                authored: radii.map(|v| v as f32),
                resolved: radii,
            }
        })
        .collect();
    cases.extend([
        Case {
            size: [8., 8.],
            authored: [6., 0., 0., 6.],
            resolved: [6., 0., 0., 6.],
        },
        Case {
            size: [12., 10.],
            authored: [12., 8., 2., 6.],
            resolved: [7.2, 4.8, 1.2, 3.6],
        },
        Case {
            size: [8., 8.],
            authored: [100., 20., 0., 0.],
            resolved: [20. / 3., 4. / 3., 0., 0.],
        },
        Case {
            size: [8., 8.],
            authored: [f32::MAX, 0., 0., 0.],
            resolved: [8., 0., 0., 0.],
        },
    ]);
    for scale in [1e-20, 1e-10, 1e20] {
        cases.push(Case {
            size: [8. * scale, 8. * scale],
            authored: [(6. * scale) as f32, 0., 0., 0.],
            resolved: [6. * scale, 0., 0., 0.],
        });
    }
    for (index, case) in cases.iter().enumerate() {
        let [tl, tr, bl, br] = case.authored;
        let shape = Sdf::Rect(SdfRect {
            start: Point::ZERO,
            end: Point::new(case.size[0], case.size[1]),
            radius: Radius {
                top_left: tl,
                top_right: tr,
                bottom_left: bl,
                bottom_right: br,
            },
        });
        // Normalize every case to an eight-pixel height, independently of its authored units.
        let scale = case.size[1] / 8.0;
        let requests: Vec<_> = inverses()
            .into_iter()
            .flat_map(|inverse| {
                let inverse = GpuAffine {
                    a: inverse.a * scale as f32,
                    b: inverse.b * scale as f32,
                    c: inverse.c * scale as f32,
                    d: inverse.d * scale as f32,
                    e: inverse.e * scale as f32,
                    f: inverse.f * scale as f32,
                };
                (-4..=52).flat_map(move |y| {
                    (-4..=52).map(move |x| Request {
                        record: 0,
                        point: [x as f32 * 0.25 + 0.03125, y as f32 * 0.25 + 0.03125],
                        inverse,
                    })
                })
            })
            .collect();
        let actual = sample(&context, &[encode_sdf(shape)], &requests)?;
        for (request, got) in requests.into_iter().zip(actual) {
            let expected = coverage(case, request);
            assert!(
                got.abs_diff(expected) <= 1,
                "case {index}, point {:?}, inverse {:?}: got {got}, expected {expected}",
                request.point,
                request.inverse
            );
        }
    }
    Ok(())
}
