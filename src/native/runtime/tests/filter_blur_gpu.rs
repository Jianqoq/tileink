use super::{four_api::Routes, reference::FilterVariant};
use crate::{
    native::runtime::{
        Result,
        compute::ComputeBatch,
        program::filter::blur::{self, Blur},
    },
    shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE},
};
#[test]
fn blur_validates_radius_source_bounds_and_axis() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([3, 3], vec![0; 36])?;
    let target = batch.texture_rgba8([3, 3], vec![0; 36])?;
    let c = FilterConfig {
        width: 3,
        height: 3,
        region_width: 3,
        region_height: 3,
        amount: 1.0,
        ..Default::default()
    };
    for stage in [Blur::Global, Blur::Shared] {
        for bad in [
            FilterConfig {
                amount: f32::NAN,
                ..c
            },
            FilterConfig {
                amount: f32::MAX,
                ..c
            },
            FilterConfig { blur_axis: 2, ..c },
            FilterConfig {
                source_x0: 2,
                source_x1: 1,
                ..c
            },
            FilterConfig { source_y1: 4, ..c },
        ] {
            assert!(blur::encode(&mut batch, stage, bad, None, source, target).is_err());
        }
    }
    assert!(batch.passes().is_empty());
    blur::encode(
        &mut batch,
        Blur::Shared,
        FilterConfig {
            region_width: 0,
            ..c
        },
        None,
        source,
        target,
    )?;
    assert!(batch.passes().is_empty());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_blur_shared_global_sparse_and_radius_boundary() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let extent = [65u32, 33];
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
    let tiles = [14, 0, 7, 5];
    let mut identities = Vec::new();
    let mut index = 0;
    for stage in [Blur::Global, Blur::Shared] {
        for axis in [0, 1] {
            for sigma in [-1.0, 0.0, 0.1, 0.4, 1.0, 4.0, 5.3, 5.34, 7.0, 8.0, 28.0] {
                for compact in [false, true] {
                    for separate_source in [false, true] {
                        let c = FilterConfig {
                            width: extent[0],
                            height: extent[1],
                            region_x0: 1,
                            region_y0: 1,
                            region_width: 64,
                            region_height: 32,
                            amount: sigma,
                            blur_axis: axis,
                            dispatch_width: 2,
                            source_x0: if separate_source { 2 } else { 0 },
                            source_y0: if separate_source { 2 } else { 0 },
                            source_x1: if separate_source { 63 } else { 0 },
                            source_y1: if separate_source { 31 } else { 0 },
                            ..Default::default()
                        };
                        let target = batch.texture_rgba8(extent, vec![57; pixels.len()])?;
                        blur::encode(
                            &mut batch,
                            stage,
                            c,
                            compact.then_some(tiles.as_slice()),
                            source,
                            target,
                        )?;
                        batch.readback(target)?;
                        if sigma <= 0.0 {
                            let mut cpu = vec![57; pixels.len()];
                            for y in 1..extent[1] {
                                for x in 1..extent[0] {
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
                            identities.push((index, cpu));
                        }
                        index += 1;
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
    for (index, cpu) in identities {
        assert_eq!(expected[index], cpu, "zero sigma identity");
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "blur global/shared corpus",
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
fn four_api_blur_matches_independent_gaussian_impulse() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let extent = [65u32, 33];
    let center = [32u32, 16u32];
    let color = [96u8, 48, 24, 192];
    let mut pixels = vec![0; 65 * 33 * 4];
    let i = ((center[1] * extent[0] + center[0]) * 4) as usize;
    pixels[i..i + 4].copy_from_slice(&color);
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8(extent, pixels)?;
    let mut expected = Vec::new();
    for stage in [Blur::Global, Blur::Shared] {
        for axis in [0, 1] {
            for sigma in [0.4f32, 1.0, 3.7, 6.0] {
                let c = FilterConfig {
                    width: extent[0],
                    height: extent[1],
                    region_width: extent[0],
                    region_height: extent[1],
                    amount: sigma,
                    blur_axis: axis,
                    ..Default::default()
                };
                let target = batch.texture_rgba8(extent, vec![57; 65 * 33 * 4])?;
                blur::encode(&mut batch, stage, c, None, source, target)?;
                batch.readback(target)?;
                let radius = (f64::from(sigma) * 3.0).ceil() as i32;
                let gaussian =
                    |d: i32| (-f64::from(d * d) / (2.0 * f64::from(sigma).powi(2))).exp();
                let total: f64 = (-radius..=radius).map(gaussian).sum();
                let mut cpu = vec![0; 65 * 33 * 4];
                for d in -radius..=radius {
                    let x = center[0] as i32 + if axis == 0 { d } else { 0 };
                    let y = center[1] as i32 + if axis == 1 { d } else { 0 };
                    if x < 0 || x >= extent[0] as i32 || y < 0 || y >= extent[1] as i32 {
                        continue;
                    }
                    let i = ((y as u32 * extent[0] + x as u32) * 4) as usize;
                    for lane in 0..4 {
                        cpu[i + lane] =
                            (f64::from(color[lane]) * gaussian(d) / total).round() as u8;
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
                "independent Gaussian impulse",
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
fn four_api_blur_discards_center_outside_source_domain() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    // The only bright pixel is outside the primitive's valid input. No tap,
    // including the center, may make that pixel part of either blur strategy.
    let source = batch.texture_rgba8([5, 1], [vec![255; 4], vec![0; 16]].concat())?;
    let mut expected = Vec::new();
    for stage in [Blur::Global, Blur::Shared] {
        let c = FilterConfig {
            width: 5,
            height: 1,
            region_width: 5,
            region_height: 1,
            source_x0: 1,
            source_x1: 4,
            source_y1: 1,
            amount: 1.0,
            ..Default::default()
        };
        let target = batch.texture_rgba8([5, 1], vec![57; 20])?;
        blur::encode(&mut batch, stage, c, None, source, target)?;
        batch.readback(target)?;
        expected.push(vec![0; 20]);
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "blur center obeys source domain",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
