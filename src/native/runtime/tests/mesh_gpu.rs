//! Known parameter points provide an oracle independent of shader quad inversion.
use super::{
    Result,
    gpu_support::{context, sample},
};
use crate::shared::{brush::push_encoded_brush, image::premul_color_to_rgba8_pack};
use crate::{Brush, MeshGradient, MeshInterpolation};
use peniko::Color;

#[path = "mesh_pixels.rs"]
mod pixels;

fn interpolate(colors: [Color; 4], mut u: f64, mut v: f64, mode: MeshInterpolation) -> u32 {
    if mode == MeshInterpolation::Smooth {
        u = u * u * (3.0 - 2.0 * u);
        v = v * v * (3.0 - 2.0 * v);
    }
    let weights = [(1.0 - u) * (1.0 - v), u * (1.0 - v), (1.0 - u) * v, u * v];
    let colors = colors.map(|c| premul_color_to_rgba8_pack(c).to_le_bytes());
    u32::from_le_bytes(std::array::from_fn(|channel| {
        colors
            .iter()
            .zip(weights)
            .map(|(c, w)| f64::from(c[channel]) * w)
            .sum::<f64>()
            .round() as u8
    }))
}

fn assert_close(actual: u32, expected: u32, label: &str) {
    assert!(
        actual
            .to_le_bytes()
            .iter()
            .zip(expected.to_le_bytes())
            .all(|(a, b)| a.abs_diff(b) <= 1),
        "{label}: actual={actual:x} expected={expected:x}"
    );
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_mesh_parameter_samples_cover_skew_winding_alpha_modes_and_bin_edges() -> Result<()> {
    let context = context()?;
    let n = 4;
    let colors: Vec<_> = (0..n * n)
        .map(|i| {
            Color::from_rgba8(
                (i * 53) as u8,
                (i * 71) as u8,
                (i * 97) as u8,
                [0, 128, 254, 255][i % 4],
            )
        })
        .collect();
    for reflected in [false, true] {
        let positions: Vec<_> = (0..n)
            .flat_map(|y| {
                (0..n).map(move |x| {
                    let (mut px, mut py) = (x as f32 * 100.0, y as f32 * 80.0);
                    if x > 0 && y > 0 && x < n - 1 && y < n - 1 {
                        px += 25.0 * (y as f32 - 1.5);
                        py += 19.0 * (x as f32 - 1.5);
                    }
                    if reflected {
                        px = 300.0 - px;
                    }
                    [px, py]
                })
            })
            .collect();
        for mode in [MeshInterpolation::Bilinear, MeshInterpolation::Smooth] {
            let mesh = MeshGradient::new(n, n, &positions, &colors)?.with_interpolation(mode);
            let mut words = vec![0; 7];
            push_encoded_brush(&mut words, &Brush::Mesh(mesh));
            let mut points = Vec::new();
            let mut expected = Vec::new();
            for y in 0..n - 1 {
                for x in 0..n - 1 {
                    let indices = [
                        y * n + x,
                        y * n + x + 1,
                        (y + 1) * n + x,
                        (y + 1) * n + x + 1,
                    ];
                    let p = indices.map(|i| positions[i]);
                    let c = indices.map(|i| colors[i]);
                    for j in 0..=12 {
                        for i in 0..=12 {
                            let (u, v) = (f64::from(i) / 12.0, f64::from(j) / 12.0);
                            let weights =
                                [(1.0 - u) * (1.0 - v), u * (1.0 - v), (1.0 - u) * v, u * v];
                            points.push(std::array::from_fn(|axis| {
                                p.iter()
                                    .zip(weights)
                                    .map(|(p, w)| f64::from(p[axis]) * w)
                                    .sum::<f64>() as f32
                            }));
                            expected.push(interpolate(c, u, v, mode));
                        }
                    }
                }
            }
            points.extend([
                [-0.01, 120.0],
                [300.01, 120.0],
                [150.0, -0.01],
                [150.0, 240.01],
                [f32::NAN, 0.0],
                [f32::INFINITY, 0.0],
            ]);
            expected.extend([0; 6]);
            let actual = sample(&context, &words, &points)?;
            for (i, (&a, &e)) in actual.iter().zip(&expected).enumerate() {
                assert_close(
                    a,
                    e,
                    &format!("reflected={reflected} mode={mode:?} point={:?}", points[i]),
                );
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_mesh_inverse_handles_nonparallelograms_and_transparent_exterior() -> Result<()> {
    let context = context()?;
    let colors = [
        Color::WHITE,
        Color::from_rgb8(255, 0, 0),
        Color::from_rgb8(0, 255, 0),
        Color::from_rgb8(0, 0, 255),
    ];
    for p in [
        [[0.0, 0.0], [100.0, 0.0], [45.0, 100.0], [55.0, 100.0]],
        [[0.0, 0.0], [100.0, 45.0], [0.0, 100.0], [100.0, 55.0]],
        [[0.0, 0.0], [100.0, 0.0], [0.0, 100.0], [1.0, 100.0]],
        [[0.0, 0.0], [100.0, 0.0], [0.0, 100.0], [0.010_002, 100.0]],
    ] {
        let mut words = vec![0; 7];
        push_encoded_brush(
            &mut words,
            &Brush::Mesh(MeshGradient::new(2, 2, p, colors)?),
        );
        let mut points = Vec::new();
        let mut expected = Vec::new();
        for j in 0..=20 {
            for i in 0..=20 {
                let (u, v) = (f64::from(i) / 20.0, f64::from(j) / 20.0);
                let weights = [(1.0 - u) * (1.0 - v), u * (1.0 - v), (1.0 - u) * v, u * v];
                points.push(std::array::from_fn(|axis| {
                    p.iter()
                        .zip(weights)
                        .map(|(p, w)| f64::from(p[axis]) * w)
                        .sum::<f64>() as f32
                }));
                expected.push(interpolate(colors, u, v, MeshInterpolation::Smooth));
            }
        }
        let actual = sample(&context, &words, &points)?;
        for (a, e) in actual.into_iter().zip(expected) {
            assert_close(a, e, "nonparallelogram");
        }
        let exterior = sample(&context, &words, &[[99.0, 99.0]])?;
        assert_eq!(exterior, [0]);
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_mesh_malformed_payloads_are_bounded_and_transparent() -> Result<()> {
    let context = context()?;
    let mesh = MeshGradient::new(
        2,
        2,
        [[0.0, 0.0], [100.0, 0.0], [0.0, 100.0], [100.0, 100.0]],
        [Color::WHITE; 4],
    )?;
    let mut words = vec![0; 7];
    push_encoded_brush(&mut words, &Brush::Mesh(mesh));
    for (word, value) in [
        (7 + 2, u32::MAX),
        (7 + 3, u32::MAX),
        (7 + 4, 0),
        (7 + 4, 33),
        (7 + 6, u32::MAX),
        (7 + 21, 0),
        (7 + 21 + 1, u32::MAX),
        (7 + 21 + 2, u32::MAX),
        (7 + 9 + 6, f32::NAN.to_bits()),
        (7 + 9 + 8, f32::INFINITY.to_bits()),
    ] {
        let mut corrupt = words.clone();
        corrupt[word] = value;
        assert_eq!(
            sample(&context, &corrupt, &[[50.0, 50.0]])?,
            [0],
            "word={word}"
        );
    }
    context.check_validation()?;
    Ok(())
}
