use super::*;
use crate::{NativeBackend, NativeContext, NativeContextOptions};
use peniko::kurbo::Point;

pub(super) fn context() -> Result<NativeContext> {
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    Ok(NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: false,
        },
    )?)
}

pub(super) fn render(
    context: &NativeContext,
    size: [u32; 2],
    input: Vec<u8>,
    blur: ProgressiveBlur,
) -> Result<Vec<u8>> {
    let mut batch = ComputeBatch::new();
    let image = batch.texture_rgba8(size, input)?;
    encode(
        &mut batch,
        image,
        size,
        Bounds::canvas(size[0], size[1]),
        blur,
    )?;
    batch.readback(image)?;
    Ok(context
        .adapter
        .submit_compute(&batch)
        .unwrap()
        .readback()?
        .remove(0))
}

#[test]
#[ignore = "requires pinned TILEINK_NATIVE_GPU"]
fn progressive_gpu_matches_clear_and_uniform_plateaus_and_gaussian_reference() -> Result<()> {
    let context = context()?;
    let size = [257u32, 193];
    let input: Vec<u8> = (0..size[0] * size[1])
        .flat_map(|i| {
            let v = if (i % size[0] / 4).is_multiple_of(2) {
                255
            } else {
                0
            };
            [v, v, v, 255]
        })
        .collect();
    let blur = ProgressiveBlur::new(Point::new(0.0, 32.0), Point::new(0.0, 160.0), 8.0);
    let output = render(&context, size, input.clone(), blur)?;
    let uniform = render(
        &context,
        size,
        input.clone(),
        ProgressiveBlur::new(Point::ZERO, Point::ZERO, 8.0),
    )?;
    assert_eq!(
        &output[..size[0] as usize * 32 * 4],
        &input[..size[0] as usize * 32 * 4]
    );
    assert_eq!(
        &output[size[0] as usize * 160 * 4..],
        &uniform[size[0] as usize * 160 * 4..]
    );
    let mut squared_error = 0.0;
    let mut count = 0;
    // Independent high-precision Gaussian oracle. Vertical stripes reduce the
    // exact 2D gather to 1D away from the transparent source boundary.
    for y in 40..153 {
        let t = (y as f64 + 0.5 - 32.0) / 128.0;
        let sigma = 8.0 * t * t * (3.0 - 2.0 * t);
        let radius = (4.0 * sigma).ceil() as i32;
        for x in 48..209 {
            let mut sum = 0.0;
            let mut weight = 0.0;
            for dx in -radius..=radius {
                let w = (-f64::from(dx * dx) / (2.0 * sigma * sigma)).exp();
                sum += w * f64::from(input[((y * 257 + (x + dx)) * 4) as usize]);
                weight += w;
            }
            let error = f64::from(output[((y * 257 + x) * 4) as usize]) - sum / weight;
            squared_error += error * error;
            count += 1;
        }
    }
    let rms = (squared_error / f64::from(count)).sqrt();
    assert!(rms < 12.0, "Gaussian reference RMS {rms} / 255");
    // Distinguish scale selection from simply fading in a fully blurred image.
    let center = ((96 * 257 + 128) * 4) as usize;
    let crossfade = (f32::from(input[center]) + f32::from(uniform[center])) * 0.5;
    assert!((f32::from(output[center]) - crossfade).abs() > 20.0);
    Ok(())
}

#[test]
#[ignore = "requires pinned TILEINK_NATIVE_GPU"]
fn progressive_gpu_preserves_premultiplication_and_handles_tiny_odd_images() -> Result<()> {
    let context = context()?;
    for quality in [
        ProgressiveBlurQuality::Balanced,
        ProgressiveBlurQuality::High,
    ] {
        for size in [[1u32, 1], [1, 19], [23, 1], [35, 27]] {
            let input: Vec<u8> = (0..size[0] * size[1])
                .flat_map(|i| {
                    let a = if i.is_multiple_of(3) { 173 } else { 0 };
                    [a, 0, 0, a]
                })
                .collect();
            for sigma in [0.0, f32::MIN_POSITIVE, 0.125, 0.25, 1.0, 4.0, 64.0, 65536.0] {
                let blur =
                    ProgressiveBlur::new(Point::new(24.0, 20.0), Point::new(2.0, 1.0), sigma)
                        .with_quality(quality);
                let output = render(&context, size, input.clone(), blur)?;
                if sigma == 0.0 {
                    assert_eq!(output, input);
                }
                for p in output.chunks_exact(4) {
                    assert_eq!(p[0], p[3]);
                    assert_eq!(&p[1..3], &[0, 0]);
                }
            }
        }
    }
    Ok(())
}
