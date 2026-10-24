use super::gpu_tests::{context, render};
use super::*;
use peniko::kurbo::Point;

// A per-edge error bound catches the shallow-blur artifacts that an image-wide
// RMS hides. Integer translations exercise every phase of the reduced grids.
#[test]
#[ignore = "requires pinned TILEINK_NATIVE_GPU"]
fn progressive_gpu_shallow_edges_match_gaussian_and_remain_stable_when_moving() -> Result<()> {
    let context = context()?;
    let size = [97, 65];
    for quality in [
        ProgressiveBlurQuality::Balanced,
        ProgressiveBlurQuality::High,
    ] {
        let mut worst = 0.0_f64;
        let mut profiles = Vec::new();
        for edge in 44..52 {
            let input = (0..size[0] * size[1])
                .flat_map(|i| {
                    let v = if i % size[0] >= edge { 255 } else { 0 };
                    [v, v, v, 255]
                })
                .collect();
            let output = render(
                &context,
                size,
                input,
                ProgressiveBlur::new(Point::new(0.0, 8.0), Point::new(0.0, 56.0), 4.0)
                    .with_quality(quality),
            )?;
            let mut profile = Vec::new();
            for y in 12..52 {
                let t = (f64::from(y) + 0.5 - 8.0) / 48.0;
                let sigma = 4.0 * t * t * (3.0 - 2.0 * t);
                let radius = (4.0 * sigma).ceil() as i32;
                for x in edge as i32 - 8..edge as i32 + 8 {
                    let mut sum = 0.0;
                    let mut weight = 0.0;
                    for dx in -radius..=radius {
                        let w = (-f64::from(dx * dx) / (2.0 * sigma * sigma)).exp();
                        sum += if x + dx >= edge as i32 {
                            255.0 * w
                        } else {
                            0.0
                        };
                        weight += w;
                    }
                    let actual = f64::from(output[((y * 97 + x) * 4) as usize]);
                    worst = worst.max((actual - sum / weight).abs());
                    profile.push(actual);
                }
            }
            profiles.push(profile);
        }
        let movement = profiles
            .iter()
            .skip(1)
            .flat_map(|p| p.iter().zip(&profiles[0]).map(|(a, b)| (a - b).abs()))
            .fold(0.0_f64, f64::max);
        eprintln!("{quality:?}: edge error {worst:.2}/255, movement {movement}/255");
        assert!(
            worst < 4.0 && movement <= 3.0,
            "shallow edge error {worst:.2}/255, integer-translation change {movement}/255"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires pinned TILEINK_NATIVE_GPU"]
fn progressive_gpu_diagonal_details_match_a_two_dimensional_gaussian() -> Result<()> {
    let context = context()?;
    let size = [65, 65];
    let input: Vec<u8> = (0_i32..65 * 65)
        .flat_map(|i| {
            let (x, y) = (i % 65, i / 65);
            let a = if x + y > 64 && (x - y).abs() > 2 {
                191
            } else {
                0
            };
            [a, 0, 0, a]
        })
        .collect();
    for quality in [
        ProgressiveBlurQuality::Balanced,
        ProgressiveBlurQuality::High,
    ] {
        let output = render(
            &context,
            size,
            input.clone(),
            ProgressiveBlur::new(Point::new(16.0, 16.0), Point::new(48.0, 48.0), 4.0)
                .with_quality(quality),
        )?;
        let mut worst = 0.0_f64;
        for y in 20..45 {
            for x in 20..45 {
                let t = ((f64::from(x + y) + 1.0 - 32.0) / 64.0).clamp(0.0, 1.0);
                let sigma = 4.0 * t * t * (3.0 - 2.0 * t);
                let radius = (4.0 * sigma).ceil() as i32;
                let mut sum = 0.0;
                let mut weight = 0.0;
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let w = (-f64::from(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp();
                        sum += w * f64::from(input[(((y + dy) * 65 + x + dx) * 4) as usize]);
                        weight += w;
                    }
                }
                let index = ((y * 65 + x) * 4) as usize;
                worst = worst.max((f64::from(output[index]) - sum / weight).abs());
                assert_eq!(output[index], output[index + 3]);
            }
        }
        eprintln!("{quality:?}: diagonal Gaussian error {worst:.2}/255");
        assert!(worst < 6.0, "{quality:?}: diagonal error {worst:.2}/255");
    }
    Ok(())
}

#[test]
#[ignore = "requires pinned TILEINK_NATIVE_GPU"]
fn progressive_gpu_strength_changes_are_continuous_at_level_boundaries() -> Result<()> {
    let context = context()?;
    let input: Vec<u8> = (0_i32..65 * 65)
        .flat_map(|i| {
            let v = if (i % 65 / 3 + i / 65 / 3) % 2 == 0 {
                255
            } else {
                0
            };
            [v, v, v, 255]
        })
        .collect();
    for quality in [
        ProgressiveBlurQuality::Balanced,
        ProgressiveBlurQuality::High,
    ] {
        let octave_steps = if quality == ProgressiveBlurQuality::Balanced {
            2.0
        } else {
            3.0
        };
        for index in 0..=6 {
            let sigma = 2.0_f32.powf(index as f32 / octave_steps);
            let mut frames = Vec::new();
            for factor in [0.999, 1.0, 1.001] {
                frames.push(render(
                    &context,
                    [65, 65],
                    input.clone(),
                    ProgressiveBlur::new(Point::ZERO, Point::ZERO, sigma * factor)
                        .with_quality(quality),
                )?);
            }
            for pair in frames.windows(2) {
                let worst = pair[0]
                    .iter()
                    .zip(&pair[1])
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                assert!(worst <= 2, "{quality:?}: sigma {sigma}, jump {worst}/255");
            }
        }
    }
    Ok(())
}
