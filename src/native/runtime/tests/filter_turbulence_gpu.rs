use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{Result, compute::ComputeBatch, program::filter::turbulence};
use crate::shared::{
    filter_config::FilterConfig,
    layer::filter::{
        TURBULENCE_GRADIENT_LEN, TURBULENCE_TABLE_LEN, TurbulenceLattice, turbulence_lattice,
    },
};
fn unit_lattice() -> TurbulenceLattice {
    let mut gradients = vec![0.0; TURBULENCE_GRADIENT_LEN];
    for pair in gradients.chunks_exact_mut(2) {
        pair[0] = 1.0;
    }
    TurbulenceLattice {
        selectors: [0; TURBULENCE_TABLE_LEN],
        gradients,
    }
}
#[test]
fn turbulence_validates_tables_coordinates_and_ownership() -> Result<()> {
    let mut batch = ComputeBatch::new();
    assert!(turbulence::upload(&mut batch, &[]).is_err());
    let mut bad = unit_lattice();
    bad.selectors[0] = 256;
    assert!(turbulence::upload(&mut batch, &[bad]).is_err());
    let mut bad = unit_lattice();
    bad.gradients[0] = f32::NAN;
    assert!(turbulence::upload(&mut batch, &[bad]).is_err());
    let tables = turbulence::upload(&mut batch, &[unit_lattice()])?;
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let c = FilterConfig {
        width: 1,
        height: 1,
        region_width: 1,
        region_height: 1,
        turbulence_scale_x: 1.0,
        turbulence_scale_y: 1.0,
        turbulence_num_octaves: 1,
        ..Default::default()
    };
    for bad in [
        FilterConfig {
            table_index: 1,
            ..c
        },
        FilterConfig {
            turbulence_scale_x: f32::NAN,
            ..c
        },
        FilterConfig {
            turbulence_base_frequency_x: f32::MAX,
            ..c
        },
        FilterConfig {
            turbulence_kind: 2,
            ..c
        },
    ] {
        assert!(turbulence::encode(&mut batch, bad, None, tables, target).is_err());
    }
    let mut foreign = ComputeBatch::new();
    let tables = turbulence::upload(&mut foreign, &[unit_lattice()])?;
    assert!(turbulence::encode(&mut batch, c, None, tables, target).is_err());
    assert!(batch.passes().is_empty());
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_turbulence_unit_gradient_oracle() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let tables = turbulence::upload(&mut batch, &[unit_lattice()])?;
    let mut expected = Vec::new();
    for transform in [-0.25, 0.25] {
        for kind in [0, 1] {
            for octaves in [0, 1, 3] {
                for zero_scale in [false, true] {
                    let c = FilterConfig {
                        width: 3,
                        height: 2,
                        region_width: 3,
                        region_height: 2,
                        turbulence_scale_x: if zero_scale { 0.0 } else { 1.0 },
                        turbulence_scale_y: 1.0,
                        turbulence_base_frequency_x: 1.0,
                        turbulence_base_frequency_y: 1.0,
                        turbulence_transform_x: transform,
                        turbulence_num_octaves: octaves,
                        turbulence_kind: kind,
                        ..Default::default()
                    };
                    let target = batch.texture_rgba8([3, 2], vec![57; 24])?;
                    turbulence::encode(&mut batch, c, None, tables, target)?;
                    batch.readback(target)?;
                    // With all gradients (1,0), noise is x - smoothstep(x). Higher octaves
                    // land at x=.5 or an integer and contribute exactly zero.
                    let rx = if transform < 0.0 { 0.25f64 } else { 0.75 };
                    let noise = if octaves == 0 {
                        0.0
                    } else {
                        rx - rx * rx * (3.0 - 2.0 * rx)
                    };
                    let value = if kind == 0 {
                        noise.abs()
                    } else {
                        noise * 0.5 + 0.5
                    };
                    let pixel = if zero_scale {
                        [0; 4]
                    } else {
                        let rgb = (value * value * 255.0).round() as u8;
                        [rgb, rgb, rgb, (value * 255.0).round() as u8]
                    };
                    expected.push(pixel.repeat(6));
                }
            }
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "unit gradient independent turbulence oracle",
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
fn four_api_turbulence_seeded_modes_regions() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let tables = turbulence::upload(
        &mut batch,
        &[turbulence_lattice(-7), turbulence_lattice(123)],
    )?;
    let tiles = [3, 0];
    for table_index in [0, 1] {
        for kind in [0, 1] {
            for stitch in [0, 1] {
                for linear in [0, 1] {
                    for octaves in [0, 1, 6] {
                        for compact in [false, true] {
                            let c = FilterConfig {
                                width: 17,
                                height: 19,
                                region_x0: 1,
                                region_y0: 2,
                                region_width: 16,
                                region_height: 17,
                                dispatch_width: 2,
                                table_index,
                                turbulence_scale_x: if table_index == 0 { 1.5 } else { -0.75 },
                                turbulence_scale_y: 2.0,
                                turbulence_base_frequency_x: if table_index == 0 {
                                    0.0
                                } else {
                                    0.13
                                },
                                turbulence_base_frequency_y: 0.21,
                                turbulence_transform_x: -2.5,
                                turbulence_transform_y: 3.25,
                                turbulence_num_octaves: octaves,
                                turbulence_kind: kind,
                                turbulence_stitch_tiles: stitch,
                                turbulence_linear_rgb: linear,
                                turbulence_tile_x: 1.0,
                                turbulence_tile_y: 2.0,
                                turbulence_tile_width: 16.0,
                                turbulence_tile_height: 17.0,
                                ..Default::default()
                            };
                            let target = batch.texture_rgba8([17, 19], vec![57; 17 * 19 * 4])?;
                            turbulence::encode(
                                &mut batch,
                                c,
                                compact.then_some(tiles.as_slice()),
                                tables,
                                target,
                            )?;
                            batch.readback(target)?;
                        }
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
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "seeded turbulence modes and regions",
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
fn four_api_turbulence_stitch_opposite_edges_are_periodic() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let tables = turbulence::upload(&mut batch, &[turbulence_lattice(123)])?;
    for (scale, transform, frequency) in [
        (1.0, 5001.25, 1.0),
        (1.0, -0.25, 0.7),
        (2.0, -0.25, 0.7),
        (-1.0, -0.25, 0.7),
    ] {
        for octaves in [1, 3] {
            let c = FilterConfig {
                width: 6,
                height: 6,
                region_width: 6,
                region_height: 6,
                turbulence_scale_x: scale,
                turbulence_scale_y: scale,
                turbulence_base_frequency_x: frequency,
                turbulence_base_frequency_y: frequency,
                turbulence_transform_x: transform,
                turbulence_transform_y: 0.25,
                turbulence_num_octaves: octaves,
                turbulence_kind: 1,
                turbulence_stitch_tiles: 1,
                turbulence_tile_x: 1.0,
                turbulence_tile_y: 1.0,
                turbulence_tile_width: 4.0,
                turbulence_tile_height: 4.0,
                ..Default::default()
            };
            let target = batch.texture_rgba8([6, 6], vec![57; 6 * 6 * 4])?;
            turbulence::encode(&mut batch, c, None, tables, target)?;
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
    for (case, pixels) in expected.iter().enumerate() {
        for lane in 1..=5 {
            let left = (lane * 6 + 1) * 4;
            let right = (lane * 6 + 5) * 4;
            let top = (6 + lane) * 4;
            let bottom = (30 + lane) * 4;
            assert_eq!(
                &pixels[left..left + 4],
                &pixels[right..right + 4],
                "stitch X period case {case} row {lane}"
            );
            assert_eq!(
                &pixels[top..top + 4],
                &pixels[bottom..bottom + 4],
                "stitch Y period case {case} column {lane}"
            );
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "independent stitch period invariant",
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
fn four_api_turbulence_constant_cases_skip_coordinate_arithmetic() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let tables = turbulence::upload(&mut batch, &[unit_lattice()])?;
    let mut expected = Vec::new();
    for kind in [0, 1] {
        for zero_octaves in [false, true] {
            let c = FilterConfig {
                width: 1,
                height: 1,
                region_width: 1,
                region_height: 1,
                turbulence_scale_x: 1.0,
                turbulence_scale_y: 1.0,
                turbulence_num_octaves: if zero_octaves { 0 } else { u32::MAX },
                turbulence_base_frequency_x: if zero_octaves { f32::MAX } else { 0.0 },
                turbulence_base_frequency_y: if zero_octaves { f32::MAX } else { 0.0 },
                turbulence_tile_width: f32::MAX,
                turbulence_tile_height: f32::MAX,
                turbulence_transform_x: f32::MAX,
                turbulence_transform_y: -f32::MAX,
                turbulence_stitch_tiles: 1,
                turbulence_kind: kind,
                ..Default::default()
            };
            let target = batch.texture_rgba8([1, 1], vec![57; 4])?;
            turbulence::encode(&mut batch, c, None, tables, target)?;
            batch.readback(target)?;
            expected.push(if kind == 0 {
                vec![0; 4]
            } else {
                vec![64, 64, 64, 128]
            });
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "constant turbulence must bypass all coordinate conversions",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
