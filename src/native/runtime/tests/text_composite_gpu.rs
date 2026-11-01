use super::*;
use crate::native::runtime::compute::ComputeBatch;
use crate::{NativeContext, NativeContextOptions};

fn linear(channel: f64) -> f64 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn encoded(channel: f64) -> f64 {
    if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

fn rgba(red: u32, green: u32, blue: u32, alpha: u32) -> u32 {
    let premul = |channel| (channel * alpha + 127) / 255;
    premul(red) | (premul(green) << 8) | (premul(blue) << 16) | (alpha << 24)
}

// Independent correcting-LUT oracle. A single transfer precedes the same
// encoded premultiplied source-over used by intermediate RGBA groups.
fn reference(destination: u32, source: u32, mask: u32, clip: u32, lcd: bool) -> u32 {
    let source_alpha = f64::from(source >> 24) / 255.0;
    if source_alpha == 0.0 || clip == 0 {
        return destination;
    }
    let foreground: [f64; 3] =
        std::array::from_fn(|i| f64::from((source >> (i * 8)) & 255) / (255.0 * source_alpha));
    let luma = foreground[0] * 0.2126 + foreground[1] * 0.7152 + foreground[2] * 0.0722;
    let raw: [u32; 3] = std::array::from_fn(|i| (((mask >> (i * 8)) & 255) * clip + 127) / 255);
    let mut coverage: [f64; 3] = std::array::from_fn(|i| {
        let value = f64::from(if lcd { raw[i] } else { raw[0] }) / 255.0;
        let f = if lcd { foreground[i] } else { luma };
        let b = 1.0 - f;
        let enhanced = value + value * (1.0 - value) * linear(b);
        if (f - b).abs() < 1.0 / 256.0 {
            enhanced
        } else {
            ((encoded(linear(f) * enhanced + linear(b) * (1.0 - enhanced)) - b) / (f - b))
                .clamp(0.0, 1.0)
        }
    });
    if destination >> 24 != 255 {
        coverage = [coverage.iter().sum::<f64>() / 3.0; 3];
    }
    let amount = source_alpha * coverage.iter().copied().fold(0.0, f64::max);
    let mut result =
        (amount * 255.0 + f64::from(destination >> 24) * (1.0 - amount)).round() as u32;
    result <<= 24;
    for (i, c) in coverage.into_iter().enumerate() {
        let value = f64::from((source >> (i * 8)) & 255) * c
            + f64::from((destination >> (i * 8)) & 255) * (1.0 - source_alpha * c);
        result |= (value.round() as u32) << (i * 8);
    }
    result
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_text_contrast_preserves_linear_color_and_coverage() -> Result<()> {
    #[cfg(feature = "dx12")]
    // SAFETY: single-threaded GPU tests enable validation before creating devices.
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let context = NativeContext::new(
        super::backend(),
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let palettes = [
        (rgba(255, 255, 255, 255), rgba(10, 10, 10, 255)),
        (rgba(10, 10, 10, 255), rgba(250, 250, 250, 255)),
        (rgba(30, 60, 110, 255), rgba(210, 230, 255, 255)),
        (rgba(240, 200, 160, 255), rgba(100, 20, 70, 128)),
        (rgba(255, 255, 255, 128), rgba(10, 10, 10, 255)),
        (0, rgba(180, 20, 50, 128)),
        (rgba(30, 60, 100, 128), 0),
    ];
    let mut cases = Vec::new();
    for (destination, source) in palettes {
        for value in 0..=255 {
            for mask in [
                value * 0x0001_0101,
                value | ((255 - value) << 8) | ((value / 2) << 16),
            ] {
                for clip in [0, 127, 255] {
                    cases.push([destination, source, mask, clip]);
                }
            }
        }
    }
    // These non-tie values expose rounding the corrected mask back to eight bits
    // before its linear-light blend. Keep that intermediate at float precision.
    let precise_start = cases.len();
    cases.extend([
        [
            rgba(255, 255, 255, 255),
            rgba(10, 10, 10, 255),
            0x008e_8e8e,
            255,
        ],
        [
            rgba(24, 24, 24, 255),
            rgba(220, 220, 220, 255),
            0x0013_1313,
            255,
        ],
    ]);
    let mut batch = ComputeBatch::new();
    let words: Vec<u32> = cases.iter().flatten().copied().collect();
    let source = batch.buffer(bytemuck::cast_slice(&words).to_vec())?;
    let output = batch.buffer(vec![0xa5; cases.len() * 20 + 4])?;
    let config = batch.buffer(bytemuck::cast_slice(&[cases.len() as u32, 0, 0, 0]).to_vec())?;
    // SAFETY: four input words and five output words are allocated per request.
    unsafe {
        batch.dispatch(
            "text_words",
            &[(9, source), (10, output), (11, config)],
            [(cases.len() as u32).div_ceil(256), 1, 1],
        )?;
    }
    batch.readback(output)?;
    let receipt = context
        .adapter
        .submit_compute(&batch)
        .map_err(|error| format!("{error:?}"))?;
    let results = receipt.readback()?;
    for (index, &[destination, source, mask, clip]) in cases.iter().enumerate() {
        for (slot, lcd) in [(2, false), (4, true)] {
            let expected = reference(destination, source, mask, clip, lcd).to_le_bytes();
            let actual = &results[0][index * 20 + slot * 4..index * 20 + slot * 4 + 4];
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(&a, b)| a.abs_diff(b) <= if index >= precise_start { 0 } else { 2 }),
                "source={source:#x}, destination={destination:#x}, mask={mask:#x}, clip={clip}, lcd={lcd}: {actual:?} != {expected:?}"
            );
        }
    }
    assert_eq!(&results[0][cases.len() * 20..], &[0xa5; 4]);
    context.check_validation()?;
    Ok(())
}
