use super::{four_api::Routes, reference::FilterVariant};
use crate::{
    native::runtime::{
        Result,
        compute::ComputeBatch,
        program::filter::rectangle::{self, RectanglePass},
    },
    shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE},
};
fn alpha(c: FilterConfig, x: u32, y: u32) -> u32 {
    let x0 = f64::from(c.rect_x0.min(c.rect_x1));
    let x1 = f64::from(c.rect_x0.max(c.rect_x1));
    let y0 = f64::from(c.rect_y0.min(c.rect_y1));
    let y1 = f64::from(c.rect_y0.max(c.rect_y1));
    let hx = (x1 - x0) / 2.0;
    let hy = (y1 - y0) / 2.0;
    let px = f64::from(x) + 0.5 - (x0 + x1) / 2.0;
    let py = f64::from(y) + 0.5 - (y0 + y1) / 2.0;
    let radius = if px >= 0.0 {
        if py <= 0.0 {
            c.radius_top_right
        } else {
            c.radius_bottom_right
        }
    } else if py > 0.0 {
        c.radius_bottom_left
    } else {
        c.radius_top_left
    };
    let r = f64::from(radius).min(hx).min(hy).max(0.0);
    let qx = px.abs() - hx + r;
    let qy = py.abs() - hy + r;
    let distance = qx.max(qy).min(0.0) + qx.max(0.0).hypot(qy.max(0.0)) - r;
    ((0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u32
}
fn over(destination: &[u8], source: &[u8], coverage: u32) -> [u8; 4] {
    let scaled: Vec<u32> = source
        .iter()
        .map(|v| (u32::from(*v) * coverage + 127) / 255)
        .collect();
    std::array::from_fn(|lane| {
        (scaled[lane] + (u32::from(destination[lane]) * (255 - scaled[3]) + 127) / 255) as u8
    })
}
#[test]
fn rectangle_validates_sdf_and_upsample_bounds() -> Result<()> {
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
        FilterConfig {
            rect_x0: f32::NAN,
            ..c
        },
        FilterConfig {
            radius_top_left: f32::INFINITY,
            ..c
        },
        FilterConfig {
            rect_x1: f32::MAX,
            ..c
        },
    ] {
        assert!(rectangle::encode(&mut batch, RectanglePass::Mask, bad, None, target).is_err());
    }
    for bad in [
        FilterConfig { source_x1: 4, ..c },
        FilterConfig {
            source_x0: 2,
            source_x1: 1,
            ..c
        },
        FilterConfig {
            upsample_filter: 2,
            ..c
        },
    ] {
        assert!(
            rectangle::encode(
                &mut batch,
                RectanglePass::UpsampleRectangle { source },
                bad,
                None,
                target
            )
            .is_err()
        );
    }
    assert!(
        rectangle::encode(
            &mut batch,
            RectanglePass::Rectangle { source: target },
            c,
            None,
            target
        )
        .is_err()
    );
    assert!(batch.passes().is_empty());
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_rectangle_masks_and_repeated_composites_match_cpu() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let extent = [33u32, 17];
    let pixels: Vec<u8> = (0..33 * 17)
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
    let masks: Vec<u8> = (0..33 * 17)
        .flat_map(|i| [(i * 29 % 256) as u8; 4])
        .collect();
    let initial = [37u8, 71, 113, 255].repeat(33 * 17);
    let tiles = [5, 0, 2];
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8(extent, pixels.clone())?;
    let mask = batch.texture_rgba8(extent, masks.clone())?;
    let mut expected = Vec::new();
    for rect in [
        [2.0, 3.0, 28.0, 14.0],
        [-2.0, -1.0, 34.0, 18.0],
        [28.0, 14.0, 2.0, 3.0],
    ] {
        for compact in [false, true] {
            for stage in 0..4 {
                let c = FilterConfig {
                    width: 33,
                    height: 17,
                    region_x0: 1,
                    region_y0: 1,
                    region_width: 32,
                    region_height: 16,
                    dispatch_width: 2,
                    rect_x0: rect[0],
                    rect_y0: rect[1],
                    rect_x1: rect[2],
                    rect_y1: rect[3],
                    radius_top_left: 0.0,
                    radius_top_right: 3.0,
                    radius_bottom_left: 6.0,
                    radius_bottom_right: 4.0,
                    mask_enabled: u32::from(stage == 2),
                    ..Default::default()
                };
                let pass = match stage {
                    0 => RectanglePass::Mask,
                    1 | 2 => RectanglePass::Direct {
                        source,
                        mask: (stage == 2).then_some(mask),
                    },
                    _ => RectanglePass::Rectangle { source },
                };
                let target = batch.texture_rgba8(extent, initial.clone())?;
                let mut cpu = initial.clone();
                for _ in 0..2 {
                    rectangle::encode(
                        &mut batch,
                        pass,
                        c,
                        compact.then_some(tiles.as_slice()),
                        target,
                    )?;
                    for y in 1..17 {
                        for x in 1..33 {
                            if compact
                                && !tiles.contains(
                                    &(y / TILE_SIZE * 33u32.div_ceil(TILE_SIZE) + x / TILE_SIZE),
                                )
                            {
                                continue;
                            }
                            let i = ((y * 33 + x) * 4) as usize;
                            let coverage = match stage {
                                0 | 3 => alpha(c, x, y),
                                2 => u32::from(masks[i + 3]),
                                _ => 255,
                            };
                            let result = if stage == 0 {
                                [coverage as u8; 4]
                            } else {
                                over(&cpu[i..i + 4], &pixels[i..i + 4], coverage)
                            };
                            cpu[i..i + 4].copy_from_slice(&result);
                        }
                    }
                }
                batch.readback(target)?;
                expected.push(cpu);
            }
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "rectangle SDF and repeated compositing CPU oracle",
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
fn four_api_upsample_rectangle_composite_constant_and_empty_domain() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let pixel = [80u8, 40, 20, 128];
    let source = batch.texture_rgba8([17, 19], pixel.repeat(17 * 19))?;
    let initial = [37u8, 71, 113, 255].repeat(17 * 19);
    let mut expected = Vec::new();
    for factor in [0, 1, 2, 3, 5] {
        for mode in [0, 1] {
            for empty in [false, true] {
                let c = FilterConfig {
                    width: 17,
                    height: 19,
                    region_width: 17,
                    region_height: 19,
                    rect_x0: 1.0,
                    rect_y0: 2.0,
                    rect_x1: 16.0,
                    rect_y1: 18.0,
                    radius_top_left: 3.0,
                    radius_top_right: 4.0,
                    radius_bottom_left: 2.0,
                    radius_bottom_right: 1.0,
                    source_x0: 1,
                    source_y0: 1,
                    source_x1: if empty { 1 } else { 12 },
                    source_y1: 8,
                    downsample: factor,
                    upsample_filter: mode,
                    ..Default::default()
                };
                let target = batch.texture_rgba8([17, 19], initial.clone())?;
                rectangle::encode(
                    &mut batch,
                    RectanglePass::UpsampleRectangle { source },
                    c,
                    None,
                    target,
                )?;
                batch.readback(target)?;
                let mut cpu = initial.clone();
                if !empty {
                    for y in 0..19 {
                        for x in 0..17 {
                            let i = ((y * 17 + x) * 4) as usize;
                            let result = over(&cpu[i..i + 4], &pixel, alpha(c, x, y));
                            cpu[i..i + 4].copy_from_slice(&result);
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
                "upsample rectangle composition oracle",
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
fn four_api_upsample_rectangle_spatial_sampling_oracle() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let pixels: Vec<u8> = (0..81u32)
        .flat_map(|i| {
            [
                (i % 9 * 12) as u8,
                (i / 9 * 12) as u8,
                ((i % 9 + i / 9) * 6) as u8,
                128,
            ]
        })
        .collect();
    let source = batch.texture_rgba8([9, 9], pixels.clone())?;
    let initial = [37, 71, 113, 255].repeat(81);
    let mut expected = Vec::new();
    for factor in [1, 2] {
        for mode in [0, 1] {
            let c = FilterConfig {
                width: 9,
                height: 9,
                region_width: 9,
                region_height: 9,
                rect_x0: 0.0,
                rect_y0: 0.0,
                rect_x1: 9.0,
                rect_y1: 9.0,
                radius_top_left: 2.0,
                radius_bottom_right: 3.0,
                source_x0: 1,
                source_y0: 2,
                source_x1: 4,
                source_y1: 5,
                downsample: factor,
                upsample_filter: mode,
                ..Default::default()
            };
            let target = batch.texture_rgba8([9, 9], initial.clone())?;
            rectangle::encode(
                &mut batch,
                RectanglePass::UpsampleRectangle { source },
                c,
                None,
                target,
            )?;
            batch.readback(target)?;
            let mut cpu = initial.clone();
            for y in 0..9 {
                for x in 0..9 {
                    let sx = ((f64::from(x) + 0.5) / f64::from(factor) - 0.5).clamp(1.0, 3.0);
                    let sy = ((f64::from(y) + 0.5) / f64::from(factor) - 0.5).clamp(2.0, 4.0);
                    let sample = |xx: u32, yy: u32, lane: usize| {
                        f64::from(pixels[((yy * 9 + xx) * 4) as usize + lane])
                    };
                    let pixel = std::array::from_fn::<_, 4, _>(|lane| {
                        if mode == 0 {
                            return sample(
                                sx.round_ties_even() as u32,
                                sy.round_ties_even() as u32,
                                lane,
                            ) as u8;
                        }
                        let bx = sx.floor() as u32;
                        let by = sy.floor() as u32;
                        let tx = sx - f64::from(bx);
                        let ty = sy - f64::from(by);
                        (sample(bx, by, lane) * (1.0 - tx) * (1.0 - ty)
                            + sample(bx + 1, by, lane) * tx * (1.0 - ty)
                            + sample(bx, by + 1, lane) * (1.0 - tx) * ty
                            + sample(bx + 1, by + 1, lane) * tx * ty)
                            .round() as u8
                    });
                    let i = ((y * 9 + x) * 4) as usize;
                    let result = over(&cpu[i..i + 4], &pixel, alpha(c, x, y));
                    cpu[i..i + 4].copy_from_slice(&result);
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
                "upsample rectangle independent spatial sampling",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
