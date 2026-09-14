use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{
    Result,
    compute::ComputeBatch,
    program::filter::glass::{self, Glass},
};
use crate::shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE};

#[test]
fn glass_rejects_invalid_parameters_and_sampling_domains() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let base = FilterConfig {
        width: 1,
        height: 1,
        region_width: 1,
        region_height: 1,
        ..Default::default()
    };
    for config in [
        FilterConfig {
            liquid_refraction_factor: f32::NAN,
            ..base
        },
        FilterConfig {
            rect_x0: f32::INFINITY,
            ..base
        },
        FilterConfig {
            source_x1: 2,
            ..base
        },
        FilterConfig {
            source_x0: 1,
            ..base
        },
        FilterConfig {
            upsample_filter: 2,
            ..base
        },
        FilterConfig {
            mask_enabled: 2,
            ..base
        },
    ] {
        assert!(
            glass::encode(
                &mut batch,
                Glass::Effect,
                config,
                None,
                source,
                source,
                target
            )
            .is_err()
        );
    }
    assert!(
        glass::encode(
            &mut batch,
            Glass::Effect,
            base,
            None,
            source,
            source,
            source
        )
        .is_err()
    );
    assert!(batch.passes().is_empty());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_glass_effects_and_rectangle_composites() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let size = [33u32, 35];
    let pixels: Vec<u8> = (0..size[0] * size[1])
        .flat_map(|i| [(i * 17) as u8, (i * 37) as u8, (i * 71) as u8, 255])
        .collect();
    let source = batch.texture_rgba8(size, pixels.clone())?;
    let blurred = batch.texture_rgba8(
        size,
        [31u8, 67, 93, 191].repeat((size[0] * size[1]) as usize),
    )?;
    let initial = [13u8, 29, 47, 127].repeat(35 * 37);
    let mut identities = Vec::new();
    let mut count = 0;
    for scenario in 0..8 {
        for compact in [false, true] {
            for mode in [Glass::Effect, Glass::RectangleComposite] {
                let target = batch.texture_rgba8([35, 37], initial.clone())?;
                let mut config = FilterConfig {
                    width: size[0],
                    height: size[1],
                    region_x0: 1,
                    region_y0: 1,
                    region_width: 31,
                    region_height: 33,
                    rect_x0: 4.25,
                    rect_y0: 3.5,
                    rect_x1: 29.0,
                    rect_y1: 31.25,
                    radius_top_left: 3.0,
                    radius_top_right: 5.0,
                    radius_bottom_left: 1.0,
                    radius_bottom_right: 7.0,
                    liquid_refraction_thickness: 6.0,
                    liquid_refraction_factor: 1.5,
                    source_x1: size[0],
                    source_y1: size[1],
                    dispatch_width: 2,
                    ..Default::default()
                };
                match scenario {
                    0 => {
                        config.rect_x0 = 1000.0;
                        config.rect_y0 = 1000.0;
                        config.rect_x1 = 1010.0;
                        config.rect_y1 = 1010.0;
                    }
                    1 => config.liquid_refraction_factor = 1.0,
                    2 => config.liquid_refraction_dispersion = 2.0,
                    3 => {
                        config.liquid_tint_r = 0.3;
                        config.liquid_tint_g = 0.8;
                        config.liquid_tint_b = 0.5;
                        config.liquid_tint_a = 0.6;
                    }
                    4 => {
                        config.liquid_fresnel_factor = 0.5;
                        config.liquid_fresnel_range = 20.0;
                        config.liquid_fresnel_hardness = 0.1;
                    }
                    5 => {
                        config.liquid_glare_factor = 0.7;
                        config.liquid_glare_range = 20.0;
                        config.liquid_glare_convergence = 0.5;
                        config.liquid_glare_opposite_factor = 0.8;
                        config.liquid_glare_angle = 0.3;
                    }
                    6 | 7 => {
                        config.downsample = 2;
                        config.upsample_filter = scenario - 6;
                        config.source_x1 = 17;
                        config.source_y1 = 18;
                        config.mask_enabled = 1;
                    }
                    _ => unreachable!(),
                }
                glass::encode(
                    &mut batch,
                    mode,
                    config,
                    compact.then_some(&[0, 4, 8]),
                    source,
                    blurred,
                    target,
                )?;
                batch.readback(target)?;
                if scenario == 0 {
                    let mut expected = initial.clone();
                    if matches!(mode, Glass::Effect) {
                        for y in 1..34u32 {
                            for x in 1..32u32 {
                                let tile = y / TILE_SIZE * 3 + x / TILE_SIZE;
                                if compact && ![0, 4, 8].contains(&tile) {
                                    continue;
                                }
                                let i = ((y * size[0] + x) * 4) as usize;
                                let o = ((y * 35 + x) * 4) as usize;
                                expected[o..o + 4].copy_from_slice(&pixels[i..i + 4]);
                            }
                        }
                    }
                    identities.push((count, expected));
                }
                count += 1;
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
        assert_eq!(expected[index], pixels, "outside glass identity");
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "liquid glass effects",
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
fn four_api_glass_alpha_extremes_and_empty_blur_domain() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let pixels: Vec<u8> = (0..17 * 19)
        .flat_map(|i| {
            let a = [0u8, 1, 127, 255][i % 4];
            [a / 2, a / 3, a / 5, a]
        })
        .collect();
    let source = batch.texture_rgba8([17, 19], pixels.clone())?;
    let blurred = batch.texture_rgba8([17, 19], pixels)?;
    for case in 0..9 {
        for mode in [Glass::Effect, Glass::RectangleComposite] {
            let target = batch.texture_rgba8([17, 19], vec![0; 17 * 19 * 4])?;
            let mut config = FilterConfig {
                width: 17,
                height: 19,
                region_width: 17,
                region_height: 19,
                rect_x0: 1.5,
                rect_y0: 1.5,
                rect_x1: 15.5,
                rect_y1: 17.5,
                liquid_refraction_factor: 1.5,
                liquid_refraction_thickness: 3.0,
                source_x1: 17,
                source_y1: 19,
                ..Default::default()
            };
            match case {
                0 => config.liquid_refraction_factor = f32::MAX,
                1 => {
                    config.liquid_refraction_factor = f32::MAX;
                    config.liquid_refraction_dispersion = 50.0;
                }
                2 => config.liquid_refraction_thickness = 0.0,
                3 => {
                    config.downsample = 2;
                    config.source_x0 = 5;
                    config.source_x1 = 5;
                }
                4 => {
                    config.downsample = 2;
                    config.source_x0 = 1;
                    config.source_y0 = 2;
                    config.source_x1 = 6;
                    config.source_y1 = 7;
                    config.upsample_filter = 1;
                }
                5 => {
                    config.rect_x1 = config.rect_x0;
                    config.rect_y1 = config.rect_y0;
                }
                6 => {
                    config.radius_top_left = 100.0;
                    config.radius_top_right = -1.0;
                    config.radius_bottom_left = 8.0;
                }
                7 => {
                    config.liquid_refraction_dispersion = -10.0;
                    config.liquid_tint_a = 1.0;
                }
                8 => {
                    config.liquid_fresnel_factor = 0.3;
                    config.liquid_fresnel_range = 0.0;
                    config.liquid_fresnel_hardness = 0.0;
                }
                _ => unreachable!(),
            }
            glass::encode(&mut batch, mode, config, None, source, blurred, target)?;
            batch.readback(target)?;
        }
    }
    let expected = routes.filter_reference_output(
        &batch,
        FilterVariant {
            portable: false,
            texture_table: false,
        },
    )?;
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "liquid glass transparent and extreme inputs",
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
fn four_api_glass_negative_highlight_base_has_zero_intensity() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let black = [0u8, 0, 0, 255].repeat(17 * 19);
    let source = batch.texture_rgba8([17, 19], black.clone())?;
    let mut expected = Vec::new();
    for glare in [false, true] {
        for hardness in [0.0, 0.5] {
            let target = batch.texture_rgba8([17, 19], black.clone())?;
            let mut config = FilterConfig {
                width: 17,
                height: 19,
                region_width: 9,
                region_height: 11,
                rect_x1: 17.0,
                rect_y1: 19.0,
                liquid_refraction_factor: 1.5,
                liquid_refraction_thickness: 100.0,
                region_x0: 4,
                region_y0: 4,
                liquid_fresnel_range: 20.0,
                liquid_glare_range: 20.0,
                ..Default::default()
            };
            if glare {
                config.liquid_glare_factor = 0.7;
                config.liquid_glare_hardness = hardness;
            } else {
                config.liquid_fresnel_factor = 0.7;
                config.liquid_fresnel_hardness = hardness;
            }
            glass::encode(
                &mut batch,
                Glass::Effect,
                config,
                None,
                source,
                source,
                target,
            )?;
            batch.readback(target)?;
            expected.push(black.clone());
        }
    }
    // clamp(base^5,0,1) is exactly zero for every negative base, independent of API pow behavior.
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "negative fifth-power highlight base",
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
fn glass_host_rejects_overflowing_angles_and_non_normalized_tint() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let base = FilterConfig {
        width: 1,
        height: 1,
        region_width: 1,
        region_height: 1,
        ..Default::default()
    };
    for config in [
        FilterConfig {
            liquid_glare_angle: f32::MAX,
            ..base
        },
        FilterConfig {
            liquid_tint_r: f32::MAX,
            ..base
        },
        FilterConfig {
            rect_x0: f32::MAX,
            rect_x1: f32::MAX,
            ..base
        },
    ] {
        assert!(
            glass::encode(
                &mut batch,
                Glass::Effect,
                config,
                None,
                source,
                source,
                target
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_glass_downsampled_blur_matches_integer_sampling_oracle() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let value = |x: u32, y: u32| (x * 4 + y * 8) % 252;
    let pixels: Vec<u8> = (0..19)
        .flat_map(|y| {
            (0..17).flat_map(move |x| {
                let v = value(x, y) as u8;
                [v, v, v, 255]
            })
        })
        .collect();
    let source = batch.texture_rgba8([17, 19], [0u8, 0, 0, 255].repeat(17 * 19))?;
    let blurred = batch.texture_rgba8([17, 19], pixels)?;
    let mut expected = Vec::new();
    for linear in [false, true] {
        let mut pixels = [111u8, 111, 111, 255].repeat(19 * 21);
        let target = batch.texture_rgba8([19, 21], pixels.clone())?;
        let config = FilterConfig {
            width: 17,
            height: 19,
            region_x0: 1,
            region_y0: 1,
            region_width: 15,
            region_height: 17,
            rect_x0: -100.0,
            rect_y0: -100.0,
            rect_x1: 100.0,
            rect_y1: 100.0,
            liquid_refraction_factor: 1.5,
            liquid_refraction_thickness: 1.0,
            downsample: 2,
            upsample_filter: u32::from(linear),
            source_x0: 2,
            source_y0: 3,
            source_x1: 9,
            source_y1: 10,
            ..Default::default()
        };
        glass::encode(
            &mut batch,
            Glass::Effect,
            config,
            None,
            source,
            blurred,
            target,
        )?;
        batch.readback(target)?;
        for y in 1..18u32 {
            for x in 1..16u32 {
                // Texel-center mapping is (pixel+0.5)/2-0.5. Work in quarter texels
                // so this oracle is independent of both shader sampling implementations.
                let sx = (2 * x - 1).clamp(2 * 4, 8 * 4);
                let sy = (2 * y - 1).clamp(3 * 4, 9 * 4);
                let v = if linear {
                    let (ix, iy, fx, fy) = (sx / 4, sy / 4, sx % 4, sy % 4);
                    (value(ix, iy) * (4 - fx) * (4 - fy)
                        + value(ix + 1, iy) * fx * (4 - fy)
                        + value(ix, iy + 1) * (4 - fx) * fy
                        + value(ix + 1, iy + 1) * fx * fy)
                        / 16
                } else {
                    value((sx + 2) / 4, (sy + 2) / 4)
                } as u8;
                let offset = ((y * 19 + x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        expected.push(pixels);
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "glass downsample logical origin and filter",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
