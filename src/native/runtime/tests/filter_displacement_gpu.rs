use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{Result, compute::ComputeBatch, program::filter::displacement};
use crate::shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE};

#[test]
fn displacement_rejects_invalid_channels_and_scales() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([3, 3], vec![0; 36])?;
    let target = batch.texture_rgba8([3, 3], vec![0; 36])?;
    let c = FilterConfig {
        width: 3,
        height: 3,
        region_width: 3,
        region_height: 3,
        ..Default::default()
    };
    for invalid in [
        FilterConfig {
            kernel_edge_mode: 4,
            ..c
        },
        FilterConfig {
            kernel_preserve_alpha: 4,
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
        assert!(displacement::encode(&mut batch, invalid, None, source, source, target).is_err());
    }
    assert!(batch.passes().is_empty());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_displacement_channels_scales_and_boundaries() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let extent = [17u32, 19];
    let source: Vec<u8> = (0..extent[0] * extent[1])
        .flat_map(|i| [(i * 17) as u8, (i * 37) as u8, (i * 71) as u8, 255])
        .collect();
    // Include non-premultiplied raw map bytes too: native coordinate guards must
    // remain safe for every RGBA8 value, including tiny/zero map alpha.
    let map: Vec<u8> = (0..extent[0] * extent[1])
        .flat_map(|i| {
            [
                (i * 13) as u8,
                (i * 29) as u8,
                (i * 43) as u8,
                (i * 59) as u8,
            ]
        })
        .collect();
    let mut batch = ComputeBatch::new();
    let input = batch.texture_rgba8(extent, source.clone())?;
    let map = batch.texture_rgba8(extent, map)?;
    let tiles = [3, 0];
    let mut identities = Vec::new();
    let mut count = 0;
    for x_channel in 0..4 {
        for y_channel in 0..4 {
            for linear in [0, 1] {
                for scale in [
                    [0.0, 0.0],
                    [0.5, -0.5],
                    [1.0, 1.0],
                    [3.0, 5.0],
                    [17.0, -31.0],
                    [f32::MAX, f32::MAX],
                ] {
                    for compact in [false, true] {
                        let c = FilterConfig {
                            width: extent[0],
                            height: extent[1],
                            region_x0: 1,
                            region_y0: 1,
                            region_width: extent[0] - 2,
                            region_height: extent[1] - 2,
                            dispatch_width: 2,
                            kernel_edge_mode: x_channel,
                            kernel_preserve_alpha: y_channel,
                            lighting_output_kind: linear,
                            amount: scale[0],
                            rect_x0: scale[1],
                            ..Default::default()
                        };
                        let target = batch.texture_rgba8(extent, vec![57; source.len()])?;
                        displacement::encode(
                            &mut batch,
                            c,
                            compact.then_some(tiles.as_slice()),
                            input,
                            map,
                            target,
                        )?;
                        batch.readback(target)?;
                        if scale == [0.0, 0.0] {
                            let mut pixels = vec![57; source.len()];
                            for y in 1..extent[1] - 1 {
                                for x in 1..extent[0] - 1 {
                                    let tile = y / TILE_SIZE * extent[0].div_ceil(TILE_SIZE)
                                        + x / TILE_SIZE;
                                    if compact && !tiles.contains(&tile) {
                                        continue;
                                    }
                                    let i = ((y * extent[0] + x) * 4) as usize;
                                    pixels[i..i + 4].copy_from_slice(&source[i..i + 4]);
                                }
                            }
                            identities.push((count, pixels));
                        }
                        count += 1;
                    }
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
    for (index, pixels) in identities {
        assert_eq!(expected[index], pixels, "zero displacement identity");
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "displacement numeric corpus",
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
fn four_api_displacement_half_pixel_rounding_and_outside_zero() -> Result<()> {
    let routes = Routes::with_features(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)?;
    let mut batch = ComputeBatch::new();
    let pixels: Vec<u8> = (0..9).flat_map(|i| [(i * 20) as u8, 0, 0, 255]).collect();
    let source = batch.texture_rgba8([3, 3], pixels.clone())?;
    let map = batch.texture_rgba8([3, 3], vec![255; 36])?;
    let mut expected = Vec::new();
    for scale in [-3.0f32, -1.0, 1.0, 3.0, f32::MAX, -f32::MAX] {
        let target = batch.texture_rgba8([3, 3], vec![0; 36])?;
        let c = FilterConfig {
            width: 3,
            height: 3,
            region_width: 3,
            region_height: 3,
            kernel_edge_mode: 3,
            kernel_preserve_alpha: 3,
            amount: scale,
            rect_x0: scale,
            ..Default::default()
        };
        displacement::encode(&mut batch, c, None, source, map, target)?;
        batch.readback(target)?;
        let mut cpu = vec![0; 36];
        for y in 0..3 {
            for x in 0..3 {
                let sx = (f64::from(x) + f64::from(scale) * 0.5).round_ties_even() as i32;
                let sy = (f64::from(y) + f64::from(scale) * 0.5).round_ties_even() as i32;
                if (0..3).contains(&sx) && (0..3).contains(&sy) {
                    let from = ((sy * 3 + sx) * 4) as usize;
                    let to = ((y * 3 + x) * 4) as usize;
                    cpu[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
                }
            }
        }
        expected.push(cpu);
    }
    routes.check_variant(
        &batch,
        &expected,
        "half-pixel rounding and zero outside",
        Some(FilterVariant {
            portable: false,
            texture_table: false,
        }),
    )?;
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_displacement_fused_coordinate_half_boundary() -> Result<()> {
    let routes = Routes::with_features(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)?;
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([8, 5], [50, 60, 70, 255].repeat(40))?;
    let map = batch.texture_rgba8([8, 5], [56, 184, 8, 136].repeat(40))?;
    let target = batch.texture_rgba8([8, 5], vec![57; 160])?;
    let c = FilterConfig {
        width: 8,
        height: 5,
        region_x0: 7,
        region_y0: 1,
        region_width: 1,
        region_height: 1,
        kernel_edge_mode: 2,
        kernel_preserve_alpha: 0,
        amount: 17.0,
        rect_x0: -31.0,
        ..Default::default()
    };
    displacement::encode(&mut batch, c, None, source, map, target)?;
    batch.readback(target)?;
    // A single explicit float32 FMA fixes the coordinate evaluation order.
    assert_eq!(
        (8.0f32 / 136.0 - 0.5).mul_add(17.0, 7.0).round_ties_even(),
        -1.0
    );
    let mut expected = vec![57; 160];
    expected[60..64].fill(0);
    routes.check_variant(
        &batch,
        &[expected],
        "fused displacement half boundary",
        Some(FilterVariant {
            portable: false,
            texture_table: false,
        }),
    )?;
    routes.validate()
}
