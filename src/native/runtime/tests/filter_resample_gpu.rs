use super::{four_api::Routes, reference::FilterVariant};
use crate::{
    native::runtime::{
        Result,
        compute::ComputeBatch,
        program::filter::resample::{self, Resample},
    },
    shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE},
};
#[test]
fn resample_rejects_invalid_rectangles_and_coordinate_overflow() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([3, 3], vec![0; 36])?;
    let target = batch.texture_rgba8([3, 3], vec![0; 36])?;
    let c = FilterConfig {
        width: 3,
        height: 3,
        region_width: 3,
        region_height: 3,
        rect_x1: 3.0,
        rect_y1: 3.0,
        ..Default::default()
    };
    for bad in [
        FilterConfig { rect_x0: -1.0, ..c },
        FilterConfig {
            rect_y0: f32::NAN,
            ..c
        },
        FilterConfig { rect_x1: 4.0, ..c },
        FilterConfig {
            rect_y1: f32::INFINITY,
            ..c
        },
        FilterConfig {
            rect_x0: 2.0,
            rect_x1: 1.0,
            ..c
        },
        FilterConfig {
            downsample_filter: 2,
            ..c
        },
    ] {
        for stage in [Resample::Downsample, Resample::Upsample] {
            assert!(resample::encode(&mut batch, stage, bad, None, source, target).is_err());
        }
    }
    assert!(
        resample::encode(
            &mut batch,
            Resample::Downsample,
            FilterConfig {
                downsample: u32::MAX,
                ..c
            },
            None,
            source,
            target
        )
        .is_err()
    );
    assert!(batch.passes().is_empty());
    resample::encode(&mut batch, Resample::Downsample, c, None, source, target)?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_resample_regions_modes_and_independent_nearest() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let extent = [17u32, 19];
    let pixels: Vec<u8> = (0..extent[0] * extent[1])
        .flat_map(|i| {
            [
                (i * 11) as u8,
                (i * 37) as u8,
                (i * 71) as u8,
                (i * 43) as u8,
            ]
        })
        .collect();
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8(extent, pixels.clone())?;
    let tiles = [3, 0];
    let mut nearest = Vec::new();
    let mut index = 0;
    for down in [false, true] {
        for factor in [0u32, 1, 2, 3, 4, 5, 8] {
            for mode in [0, 1] {
                for compact in [false, true] {
                    let c = FilterConfig {
                        width: extent[0],
                        height: extent[1],
                        region_width: extent[0],
                        region_height: extent[1],
                        rect_x0: 1.0,
                        rect_y0: 2.0,
                        rect_x1: 15.0,
                        rect_y1: 18.0,
                        downsample: factor,
                        downsample_filter: mode,
                        upsample_filter: mode,
                        dispatch_width: 2,
                        ..Default::default()
                    };
                    let target = batch.texture_rgba8(extent, vec![57; pixels.len()])?;
                    resample::encode(
                        &mut batch,
                        if down {
                            Resample::Downsample
                        } else {
                            Resample::Upsample
                        },
                        c,
                        compact.then_some(tiles.as_slice()),
                        source,
                        target,
                    )?;
                    batch.readback(target)?;
                    if mode == 0 {
                        let mut cpu = vec![57; pixels.len()];
                        let f = factor.max(1);
                        for y in 0..extent[1] {
                            for x in 0..extent[0] {
                                if compact
                                    && !tiles.contains(
                                        &(y / TILE_SIZE * extent[0].div_ceil(TILE_SIZE)
                                            + x / TILE_SIZE),
                                    )
                                {
                                    continue;
                                }
                                let i = ((y * extent[0] + x) * 4) as usize;
                                let (sx, sy) = if down {
                                    let x0 = (x * f).max(1);
                                    let x1 = ((x + 1) * f).min(15);
                                    let y0 = (y * f).max(2);
                                    let y1 = ((y + 1) * f).min(18);
                                    if x0 >= x1 || y0 >= y1 {
                                        cpu[i..i + 4].fill(0);
                                        continue;
                                    }
                                    ((x0 + x1 - 1) / 2, (y0 + y1 - 1) / 2)
                                } else {
                                    let sx = ((f64::from(x) + 0.5) / f64::from(f) - 0.5)
                                        .clamp(1.0, 14.0)
                                        .round_ties_even()
                                        as u32;
                                    let sy = ((f64::from(y) + 0.5) / f64::from(f) - 0.5)
                                        .clamp(2.0, 17.0)
                                        .round_ties_even()
                                        as u32;
                                    (sx, sy)
                                };
                                let from = ((sy * extent[0] + sx) * 4) as usize;
                                cpu[i..i + 4].copy_from_slice(&pixels[from..from + 4]);
                            }
                        }
                        nearest.push((index, cpu));
                    }
                    index += 1;
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
    for (index, cpu) in nearest {
        assert_eq!(expected[index], cpu, "nearest spatial oracle {index}");
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "resample mode corpus",
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
fn four_api_resample_average_and_linear_math() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let pixels = vec![
        0u8, 20, 40, 60, 80, 100, 120, 140, 160, 180, 200, 220, 240, 220, 200, 180,
    ];
    let source = batch.texture_rgba8([4, 1], pixels)?;
    let mut expected = Vec::new();
    for stage in [Resample::Downsample, Resample::Upsample] {
        let c = FilterConfig {
            width: 4,
            height: 1,
            region_width: 4,
            region_height: 1,
            rect_x1: 4.0,
            rect_y1: 1.0,
            downsample: 2,
            downsample_filter: 1,
            upsample_filter: 1,
            ..Default::default()
        };
        let target = batch.texture_rgba8([4, 1], vec![57; 16])?;
        resample::encode(&mut batch, stage, c, None, source, target)?;
        batch.readback(target)?;
        // Exact box means and quarter-texel interpolation of a piecewise linear ramp.
        expected.push(match stage {
            Resample::Downsample => {
                vec![40, 60, 80, 100, 200, 200, 200, 200, 0, 0, 0, 0, 0, 0, 0, 0]
            }
            Resample::Upsample => vec![
                0, 20, 40, 60, 20, 40, 60, 80, 60, 80, 100, 120, 100, 120, 140, 160,
            ],
        });
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "resample independent ramp oracle",
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
fn four_api_resample_fractional_rectangles_and_two_dimensional_oracle() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut expected = Vec::new();
    for extent in [[3u32, 3u32], [1, 1]] {
        let pixels: Vec<u8> = (0..extent[0] * extent[1])
            .flat_map(|i| {
                [
                    (i * 24) as u8,
                    (i * 20 + 10) as u8,
                    (i * 12 + 30) as u8,
                    (i * 16 + 60) as u8,
                ]
            })
            .collect();
        let source = batch.texture_rgba8(extent, pixels.clone())?;
        let rectangles = if extent[0] == 1 {
            vec![[0.0, 0.0, 1.0, 1.0]]
        } else {
            vec![
                [0.0, 0.0, 3.0, 3.0],
                [0.75, 0.25, 2.75, 2.5],
                [1.25, 1.25, 1.75, 1.75],
            ]
        };
        for rect in rectangles {
            for stage in [Resample::Downsample, Resample::Upsample] {
                let c = FilterConfig {
                    width: extent[0],
                    height: extent[1],
                    region_width: extent[0],
                    region_height: extent[1],
                    rect_x0: rect[0],
                    rect_y0: rect[1],
                    rect_x1: rect[2],
                    rect_y1: rect[3],
                    downsample: 2,
                    downsample_filter: 1,
                    upsample_filter: 1,
                    ..Default::default()
                };
                let target = batch.texture_rgba8(extent, vec![57; pixels.len()])?;
                resample::encode(&mut batch, stage, c, None, source, target)?;
                batch.readback(target)?;
                let bounds = rect.map(|v| v as u32);
                let mut cpu = vec![57; pixels.len()];
                for y in 0..extent[1] {
                    for x in 0..extent[0] {
                        let i = ((y * extent[0] + x) * 4) as usize;
                        if matches!(stage, Resample::Downsample) {
                            let x0 = (x * 2).max(bounds[0]);
                            let y0 = (y * 2).max(bounds[1]);
                            let x1 = ((x + 1) * 2).min(bounds[2]);
                            let y1 = ((y + 1) * 2).min(bounds[3]);
                            if x0 >= x1 || y0 >= y1 {
                                cpu[i..i + 4].fill(0);
                                continue;
                            }
                            for lane in 0..4 {
                                let mut sum = 0u32;
                                for sy in y0..y1 {
                                    for sx in x0..x1 {
                                        sum += u32::from(
                                            pixels[((sy * extent[0] + sx) * 4) as usize + lane],
                                        );
                                    }
                                }
                                let count = (x1 - x0) * (y1 - y0);
                                cpu[i + lane] = ((sum + count / 2) / count) as u8;
                            }
                        } else {
                            if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
                                continue;
                            }
                            let sx = ((f64::from(x) + 0.5) / 2.0 - 0.5)
                                .clamp(f64::from(bounds[0]), f64::from(bounds[2] - 1));
                            let sy = ((f64::from(y) + 0.5) / 2.0 - 0.5)
                                .clamp(f64::from(bounds[1]), f64::from(bounds[3] - 1));
                            let bx = sx.floor() as u32;
                            let by = sy.floor() as u32;
                            let tx = sx - f64::from(bx);
                            let ty = sy - f64::from(by);
                            for lane in 0..4 {
                                let sample = |xx: u32, yy: u32| {
                                    f64::from(
                                        pixels[((yy.min(extent[1] - 1) * extent[0]
                                            + xx.min(extent[0] - 1))
                                            * 4)
                                            as usize
                                            + lane],
                                    )
                                };
                                let value = sample(bx, by) * (1.0 - tx) * (1.0 - ty)
                                    + sample(bx + 1, by) * tx * (1.0 - ty)
                                    + sample(bx, by + 1) * (1.0 - tx) * ty
                                    + sample(bx + 1, by + 1) * tx * ty;
                                cpu[i + lane] = value.round() as u8;
                            }
                        }
                    }
                }
                expected.push(cpu);
            }
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "resample fractional rectangle and 2D oracle",
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
fn four_api_resample_half_byte_box_means() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut pixels = Vec::new();
    let mut expected = vec![0; 512 * 4];
    for i in 0..256usize {
        let a = [i as u8, (i * 17) as u8, (i * 71) as u8, 255];
        let b = [
            i.saturating_add(1).min(255) as u8,
            (i * 31 + 1) as u8,
            (i * 43 + 2) as u8,
            255,
        ];
        pixels.extend(a);
        pixels.extend(b);
        for lane in 0..4 {
            expected[i * 4 + lane] = (u16::from(a[lane]) + u16::from(b[lane])).div_ceil(2) as u8;
        }
    }
    let source = batch.texture_rgba8([512, 1], pixels)?;
    let target = batch.texture_rgba8([512, 1], vec![0; 512 * 4])?;
    let config = FilterConfig {
        width: 512,
        height: 1,
        region_width: 512,
        region_height: 1,
        rect_x1: 512.0,
        rect_y1: 1.0,
        downsample: 2,
        downsample_filter: 1,
        ..Default::default()
    };
    resample::encode(
        &mut batch,
        Resample::Downsample,
        config,
        None,
        source,
        target,
    )?;
    batch.readback(target)?;
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                std::slice::from_ref(&expected),
                "box mean half-byte oracle",
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
fn four_api_resample_blur_chain_large_coordinates() -> Result<()> {
    use crate::native::runtime::program::filter::blur::{self, Blur};
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let extent = [1024, 64];
    let pixels: Vec<u8> = (0..extent[0] * extent[1])
        .flat_map(|i| {
            [
                (i * 17) as u8,
                (i * 31 + 37) as u8,
                (i * 71 + 113) as u8,
                255,
            ]
        })
        .collect();
    let mut source = batch.texture_rgba8(extent, pixels.clone())?;
    let low = FilterConfig {
        width: extent[0],
        height: extent[1],
        region_width: extent[0] / 4,
        region_height: extent[1] / 4,
        rect_x1: extent[0] as f32,
        rect_y1: extent[1] as f32,
        downsample: 4,
        downsample_filter: 1,
        upsample_filter: 1,
        amount: 7.0,
        ..Default::default()
    };
    for stage in 0..4 {
        let target = batch.texture_rgba8(extent, vec![0; (extent[0] * extent[1] * 4) as usize])?;
        match stage {
            0 => resample::encode(&mut batch, Resample::Downsample, low, None, source, target)?,
            1 | 2 => blur::encode(
                &mut batch,
                Blur::Global,
                FilterConfig {
                    blur_axis: stage - 1,
                    ..low
                },
                None,
                source,
                target,
            )?,
            _ => resample::encode(
                &mut batch,
                Resample::Upsample,
                FilterConfig {
                    region_width: extent[0],
                    region_height: extent[1],
                    rect_x1: low.region_width as f32,
                    rect_y1: low.region_height as f32,
                    ..low
                },
                None,
                source,
                target,
            )?,
        }
        batch.readback(target)?;
        source = target;
    }
    let expected = routes.filter_reference_output(
        &batch,
        FilterVariant {
            portable: false,
            texture_table: false,
        },
    )?;
    let mut mean = vec![0u8; pixels.len()];
    for y in 0..low.region_height {
        for x in 0..low.region_width {
            for lane in 0..4 {
                let mut sum = 0u32;
                for sy in y * 4..(y + 1) * 4 {
                    for sx in x * 4..(x + 1) * 4 {
                        sum += u32::from(pixels[((sy * extent[0] + sx) * 4) as usize + lane]);
                    }
                }
                mean[((y * extent[0] + x) * 4) as usize + lane] = ((sum + 8) / 16) as u8;
            }
        }
    }
    assert_eq!(
        expected[0], mean,
        "box stage must match the integer mean oracle"
    );
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "downsample/blur-X/blur-Y/upsample stage",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
