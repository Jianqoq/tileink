//! Angular reference uses wide periodic arithmetic, independent of GPU normalization.
use super::{
    Result,
    gpu_support::{context, sample},
};
use crate::{Brush, shared::brush::push_encoded_brush};
use peniko::{Color, Extend, Gradient, InterpolationAlphaSpace, color::ColorSpaceTag};

fn check_samples(context: &crate::NativeContext, cases: &[(f32, f32)]) -> Result<()> {
    let points: [[f32; 2]; 10] = [
        [1.0, 0.0],
        [0.0, 1.0],
        [-1.0, 0.0],
        [0.0, -1.0],
        [1.0, 1.0],
        [-1.0, 1.0],
        [-1.0, -1.0],
        [1.0, -1.0],
        [0.2, -0.7],
        [0.0, 0.0],
    ];
    for &(start, end) in cases {
        for extend in [Extend::Pad, Extend::Repeat, Extend::Reflect] {
            let mut gradient = Gradient::new_sweep((0.0, 0.0), start, end)
                .with_stops([Color::BLACK, Color::WHITE])
                .with_extend(extend);
            gradient.interpolation_cs = ColorSpaceTag::Srgb;
            gradient.interpolation_alpha_space = InterpolationAlphaSpace::Premultiplied;
            let brush = Brush::from_gradient_with_ramp_size(&gradient, 256);
            let mut words = vec![0; 7];
            push_encoded_brush(&mut words, &brush);
            let actual = sample(context, &words, &points)?;
            for (point, actual) in points.iter().zip(actual) {
                let span = f64::from(end) - f64::from(start);
                let t = if span.abs() <= f64::from(f32::EPSILON) {
                    0.0
                } else {
                    let angle = if *point == [0.0, 0.0] {
                        0.0
                    } else {
                        f64::from(point[1]).atan2(f64::from(point[0]))
                    };
                    // Reduce the start separately: subtracting a huge start from
                    // the sample direction first would discard the direction.
                    let relative = angle - f64::from(start).rem_euclid(std::f64::consts::TAU);
                    let distance = if span > 0.0 {
                        relative.rem_euclid(std::f64::consts::TAU)
                    } else {
                        -(-relative).rem_euclid(std::f64::consts::TAU)
                    };
                    distance / span
                };
                let extended = match extend {
                    Extend::Pad => t.clamp(0.0, 1.0),
                    Extend::Repeat => t.rem_euclid(1.0),
                    Extend::Reflect => {
                        let phase = t.rem_euclid(2.0);
                        phase.min(2.0 - phase)
                    }
                };
                let gray = (extended * 255.0).round() as u8;
                let expected = [gray, gray, gray, 255];
                assert!(
                    actual
                        .to_le_bytes()
                        .iter()
                        .zip(expected)
                        .all(|(a, b)| a.abs_diff(b) <= 1),
                    "start={start} end={end} extend={extend:?} point={point:?}: actual={actual:x} expected={expected:?}"
                );
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sweep_is_periodic_for_positive_negative_and_reversed_angles() -> Result<()> {
    let context = context()?;
    check_samples(
        &context,
        &[
            (0.0, std::f32::consts::TAU),
            (-7.0, -3.0),
            (7.0, 3.0),
            (10.0, 10.5),
            (-10.0, -10.5),
            (10000.0, 10006.0),
            (-10000.0, -10006.0),
            (0.0, -std::f32::consts::TAU),
            (3.0, 3.0),
            (0.0, f32::EPSILON / 2.0),
        ],
    )?;
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sweep_large_finite_angles_finish_with_correct_samples() -> Result<()> {
    let context = context()?;
    // These unrestricted inputs must only run after the bounded implementation:
    // the retired TAU loop could stall forever when float increments stop changing.
    check_samples(
        &context,
        &[
            (1.0e30, 1.1e30),
            (-1.0e30, -1.1e30),
            (-f32::MAX, f32::MAX),
            (f32::MAX, -f32::MAX),
        ],
    )?;
    context.check_validation()?;
    Ok(())
}
