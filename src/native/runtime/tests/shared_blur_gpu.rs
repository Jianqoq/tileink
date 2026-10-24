//! Independent Gaussian oracle plus exact dense/sparse history checks.
use super::super::{
    Result,
    compute::ComputeBatch,
    program::filter::blur::{self, Blur},
};
use crate::{NativeContext, NativeContextOptions, TILE_SIZE, shared::filter_config::FilterConfig};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shared_blur_preserves_partial_groups_and_sparse_history() -> Result<()> {
    #[cfg(feature = "dx12")]
    // SAFETY: serial GPU tests enable validation before creating the device.
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
    for [width, height] in [[1u32, 1u32], [17, 15], [49, 35]] {
        let columns = width.div_ceil(TILE_SIZE);
        let active: Vec<_> = (0..columns * height.div_ceil(TILE_SIZE))
            .rev()
            .filter(|tile| tile % 3 != 1)
            .collect();
        let source: Vec<u8> = (0..width * height)
            .flat_map(|i| {
                let alpha = (i * 43 % 256) as u8;
                let modulus = u32::from(alpha) + 1;
                [
                    (i * 11 % modulus) as u8,
                    (i * 37 % modulus) as u8,
                    (i * 71 % modulus) as u8,
                    alpha,
                ]
            })
            .collect();
        for amount in [
            0.0f32,
            0.01,
            1.0 / 3.0,
            0.5,
            1.0,
            3.25,
            5.0,
            16.0 / 3.0,
            5.5,
        ] {
            for axis in [0, 1] {
                for inset in [false, true] {
                    let c = FilterConfig {
                        width,
                        height,
                        region_x0: if inset { width / 3 } else { 0 },
                        region_y0: if inset { height / 3 } else { 0 },
                        region_width: width - if inset { 2 * (width / 3) } else { 0 },
                        region_height: height - if inset { 2 * (height / 3) } else { 0 },
                        // Source and output rectangles deliberately differ.
                        source_x0: width / 5,
                        source_y0: height / 5,
                        source_x1: width,
                        source_y1: height,
                        amount,
                        blur_axis: axis,
                        ..Default::default()
                    };
                    let untouched = [29u8, 31, 37, 113];
                    let mut batch = ComputeBatch::new();
                    let input = batch.texture_rgba8([width, height], source.clone())?;
                    for tiles in [None, Some(active.as_slice())] {
                        let target = batch.texture_rgba8(
                            [width, height],
                            untouched.repeat((width * height) as usize),
                        )?;
                        blur::encode(&mut batch, Blur::Shared, c, tiles, input, target)?;
                        batch.readback(target)?;
                    }
                    let receipt = context
                        .adapter
                        .submit_compute(&batch)
                        .map_err(|e| format!("{e:?}"))?;
                    drop(batch);
                    let outputs = receipt.readback()?;
                    assert_eq!(outputs.len(), 2);
                    for y in 0..height {
                        for x in 0..width {
                            let index = ((y * width + x) * 4) as usize;
                            let inside = x >= c.region_x0
                                && y >= c.region_y0
                                && x < c.region_x0 + c.region_width
                                && y < c.region_y0 + c.region_height;
                            let tile = y / TILE_SIZE * columns + x / TILE_SIZE;
                            for (sparse, output) in outputs.iter().enumerate() {
                                let actual = &output[index..index + 4];
                                if !inside || (sparse == 1 && !active.contains(&tile)) {
                                    assert_eq!(actual, untouched, "untouched x={x} y={y}");
                                    continue;
                                }
                                if sparse == 1 {
                                    assert_eq!(actual, &outputs[0][index..index + 4]);
                                }
                                let expected = gaussian(&source, c, x, y);
                                for channel in 0..4 {
                                    assert!(
                                        actual[channel].abs_diff(expected[channel]) <= 1,
                                        "size={width}x{height} sigma={amount} axis={axis} inset={inset} xy={x},{y} channel={channel}: {actual:?} vs {expected:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

// Evaluate exp(-d² / 2σ²) independently in f64, rather than copying the shader's
// rounded recurrence. Allow one byte for exp/FMA rounding; inactive pixels and
// dense/sparse equality above remain exact.
fn gaussian(source: &[u8], c: FilterConfig, x: u32, y: u32) -> [u8; 4] {
    if c.amount <= 0.0 {
        return source[((y * c.width + x) * 4) as usize..][..4]
            .try_into()
            .unwrap();
    }
    let sigma = f64::from(c.amount);
    let radius = (c.amount * 3.0).ceil().max(1.0) as i32;
    let mut sum = [0.0f64; 4];
    let mut total = 0.0;
    for distance in -radius..=radius {
        let weight = (-f64::from(distance * distance) / (2.0 * sigma * sigma)).exp();
        total += weight;
        let sx = x as i32 + if c.blur_axis == 0 { distance } else { 0 };
        let sy = y as i32 + if c.blur_axis == 1 { distance } else { 0 };
        if sx < c.source_x0 as i32
            || sy < c.source_y0 as i32
            || sx >= c.source_x1 as i32
            || sy >= c.source_y1 as i32
        {
            continue;
        }
        let index = ((sy as u32 * c.width + sx as u32) * 4) as usize;
        for channel in 0..4 {
            sum[channel] += weight * f64::from(source[index + channel]);
        }
    }
    sum.map(|value| (value / total).round().clamp(0.0, 255.0) as u8)
}
