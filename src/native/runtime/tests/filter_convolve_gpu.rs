use super::{four_api::Routes, reference::FilterVariant};
use crate::{
    native::runtime::{Result, compute::ComputeBatch, program::filter::convolve},
    shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE},
};

#[test]
fn convolution_validates_storage_shapes_coordinates_and_modes() -> Result<()> {
    let mut batch = ComputeBatch::new();
    for data in [&[][..], &[f32::NAN], &[f32::INFINITY], &[f32::MAX]] {
        assert!(convolve::upload(&mut batch, data).is_err());
    }
    let kernels = convolve::upload(&mut batch, &[1.0; 9])?;
    let source = batch.texture_rgba8([3, 3], vec![0; 36])?;
    let target = batch.texture_rgba8([3, 3], vec![0; 36])?;
    let c = FilterConfig {
        width: 3,
        height: 3,
        region_width: 3,
        region_height: 3,
        kernel_columns: 3,
        kernel_rows: 3,
        kernel_target_x: 1,
        kernel_target_y: 1,
        amount: 1.0,
        ..Default::default()
    };
    for bad in [
        FilterConfig {
            kernel_offset: 1,
            ..c
        },
        FilterConfig {
            kernel_columns: u32::MAX,
            ..c
        },
        FilterConfig {
            kernel_target_x: 3,
            ..c
        },
        FilterConfig {
            kernel_target_y: 3,
            ..c
        },
        FilterConfig {
            kernel_edge_mode: 3,
            ..c
        },
        FilterConfig {
            kernel_preserve_alpha: 2,
            ..c
        },
        FilterConfig {
            amount: f32::NAN,
            ..c
        },
        FilterConfig {
            rect_x0: f32::INFINITY,
            ..c
        },
    ] {
        assert!(convolve::encode(&mut batch, bad, None, kernels, source, target).is_err());
    }
    let mut foreign = ComputeBatch::new();
    let foreign = convolve::upload(&mut foreign, &[1.0; 9])?;
    assert!(convolve::encode(&mut batch, c, None, foreign, source, target).is_err());
    assert!(convolve::encode(&mut batch, c, None, kernels, source, source).is_err());
    assert!(batch.passes().is_empty());
    convolve::encode(&mut batch, c, None, kernels, source, target)?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_convolution_numeric_corpus_and_sparse_regions() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let extent = [17u32, 19];
    let pixels: Vec<u8> = (0..extent[0] * extent[1])
        .flat_map(|i| {
            let a = i * 43 % 256;
            let d = a + 1;
            [
                (i * 11 % d) as u8,
                (i * 37 % d) as u8,
                (i * 71 % d) as u8,
                a as u8,
            ]
        })
        .collect();
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8(extent, pixels.clone())?;
    let weights = [
        0.7f32, 1.0, -2.0, 0.25, 0.0, 3.0, -0.75, 0.125, 0.25, -0.5, 2.0,
    ];
    let kernels = convolve::upload(&mut batch, &weights)?;
    let tiles = [3, 0];
    let mut identities = Vec::new();
    let mut count = 0;
    for edge in 0..3 {
        for preserve in 0..2 {
            for divisor in [0.0, 1.0, -3.0, 2.75] {
                for compact in [false, true] {
                    let c = FilterConfig {
                        width: extent[0],
                        height: extent[1],
                        region_x0: 1,
                        region_y0: 2,
                        region_width: 15,
                        region_height: 16,
                        dispatch_width: 2,
                        kernel_offset: 1,
                        kernel_columns: 3,
                        kernel_rows: 3,
                        kernel_target_x: 0,
                        kernel_target_y: 2,
                        kernel_edge_mode: edge,
                        kernel_preserve_alpha: preserve,
                        amount: divisor,
                        rect_x0: 0.125,
                        ..Default::default()
                    };
                    let target = batch.texture_rgba8(extent, vec![57; pixels.len()])?;
                    convolve::encode(
                        &mut batch,
                        c,
                        compact.then_some(tiles.as_slice()),
                        kernels,
                        source,
                        target,
                    )?;
                    batch.readback(target)?;
                    if divisor == 0.0 {
                        let mut cpu = vec![57; pixels.len()];
                        for y in 2..18 {
                            for x in 1..16 {
                                if compact
                                    && !tiles.contains(
                                        &(y / TILE_SIZE * extent[0].div_ceil(TILE_SIZE)
                                            + x / TILE_SIZE),
                                    )
                                {
                                    continue;
                                }
                                let i = ((y * extent[0] + x) * 4) as usize;
                                cpu[i..i + 4].copy_from_slice(&pixels[i..i + 4]);
                            }
                        }
                        identities.push((count, cpu));
                    }
                    count += 1;
                }
            }
        }
    }
    let expected = routes.filter_reference_output(
        &batch,
        FilterVariant {
            portable: false,
            texture_table: false,
        },
    )?;
    for (index, cpu) in identities {
        assert_eq!(expected[index], cpu, "zero divisor copy");
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "convolution numeric corpus",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_convolution_reverses_kernel_and_wraps_narrow_region() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let pixels: Vec<u8> = (0..25)
        .flat_map(|i| [(i * 7) as u8, (i * 5) as u8, (i * 3) as u8, 255])
        .collect();
    let source = batch.texture_rgba8([5, 5], pixels.clone())?;
    // Two off-centre impulses check both wrap signs in a non-power-of-two region.
    let mut expected = Vec::new();
    for (index, dx, dy) in [(0, 3, 2), (19, -1, -1)] {
        let mut weights = vec![0.0; 20];
        weights[index] = 1.0;
        let kernels = convolve::upload(&mut batch, &weights)?;
        for edge in 0..3 {
            let c = FilterConfig {
                width: 5,
                height: 5,
                region_x0: 1,
                region_y0: 1,
                region_width: 3,
                region_height: 2,
                kernel_columns: 5,
                kernel_rows: 4,
                kernel_target_x: 1,
                kernel_target_y: 1,
                kernel_edge_mode: edge,
                amount: 1.0,
                ..Default::default()
            };
            let target = batch.texture_rgba8([5, 5], vec![57; 100])?;
            convolve::encode(&mut batch, c, None, kernels, source, target)?;
            batch.readback(target)?;
            let mut cpu = vec![57; 100];
            for y in 1i32..3 {
                for x in 1i32..4 {
                    let mut sx = x + dx;
                    let mut sy = y + dy;
                    let i = ((y * 5 + x) * 4) as usize;
                    if edge == 0 && (!(1..4).contains(&sx) || !(1..3).contains(&sy)) {
                        cpu[i..i + 4].fill(0);
                        continue;
                    }
                    if edge == 1 {
                        sx = sx.clamp(1, 3);
                        sy = sy.clamp(1, 2);
                    }
                    if edge == 2 {
                        sx = 1 + (sx - 1).rem_euclid(3);
                        sy = 1 + (sy - 1).rem_euclid(2);
                    }
                    let from = ((sy * 5 + sx) * 4) as usize;
                    cpu[i..i + 4].copy_from_slice(&pixels[from..from + 4]);
                }
            }
            expected.push(cpu);
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "convolution reversed impulse oracle",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_convolution_straight_alpha_bias_oracle() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let pixels = vec![
        0u8, 0, 0, 0, 32, 64, 16, 128, 30, 60, 90, 120, 255, 100, 50, 255,
    ];
    let source = batch.texture_rgba8([4, 1], pixels.clone())?;
    let kernels = convolve::upload(&mut batch, &[0.25, 0.75])?;
    let mut expected = Vec::new();
    for preserve in [0, 1] {
        let c = FilterConfig {
            width: 4,
            height: 1,
            region_width: 4,
            region_height: 1,
            kernel_columns: 2,
            kernel_rows: 1,
            kernel_edge_mode: 1,
            kernel_preserve_alpha: preserve,
            amount: 1.25,
            rect_x0: 0.125,
            ..Default::default()
        };
        let target = batch.texture_rgba8([4, 1], vec![0; 16])?;
        convolve::encode(&mut batch, c, None, kernels, source, target)?;
        batch.readback(target)?;
        let mut cpu = vec![0; 16];
        for x in 0..4 {
            let left = &pixels[x * 4..x * 4 + 4];
            let right_x = (x + 1).min(3);
            let right = &pixels[right_x * 4..right_x * 4 + 4];
            let alpha = if preserve == 1 {
                f64::from(left[3]) / 255.0
            } else {
                ((f64::from(left[3]) * 0.75 + f64::from(right[3]) * 0.25) / 255.0 / 1.25 + 0.125)
                    .clamp(0.0, 1.0)
            };
            for channel in 0..3 {
                let straight = |p: &[u8]| {
                    if p[3] == 0 {
                        0.0
                    } else {
                        f64::from(p[channel]) / f64::from(p[3])
                    }
                };
                let value = ((straight(left) * 0.75 + straight(right) * 0.25) / 1.25 + 0.125)
                    .clamp(0.0, 1.0);
                cpu[x * 4 + channel] = (value * alpha * 255.0).round() as u8;
            }
            cpu[x * 4 + 3] = (alpha * 255.0).round() as u8;
        }
        expected.push(cpu);
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "convolution independent straight-alpha oracle",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
