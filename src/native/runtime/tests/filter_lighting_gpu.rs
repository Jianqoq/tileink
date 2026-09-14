use super::{four_api::Routes, reference::FilterVariant};
use crate::{
    native::runtime::{Result, compute::ComputeBatch, program::filter::lighting},
    shared::filter_config::FilterConfig,
};
#[test]
fn lighting_rejects_nonfinite_and_overflowing_directions() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let c = FilterConfig {
        width: 1,
        height: 1,
        region_width: 1,
        region_height: 1,
        ..Default::default()
    };
    for bad in [
        FilterConfig { light_kind: 3, ..c },
        FilterConfig {
            lighting_output_kind: 2,
            ..c
        },
        FilterConfig {
            surface_scale: f32::MAX,
            ..c
        },
        FilterConfig {
            light_p2: f32::MAX,
            ..c
        },
        FilterConfig {
            light_r: f32::NAN,
            ..c
        },
        FilterConfig {
            light_p7: f32::INFINITY,
            ..c
        },
    ] {
        assert!(lighting::encode(&mut batch, bad, None, source, target).is_err());
    }
    assert!(batch.passes().is_empty());
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_lighting_modes_sparse_edges_and_spot_cones() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let extent = [17u32, 19];
    let pixels: Vec<u8> = (0..extent[0] * extent[1])
        .flat_map(|i| [0, 0, 0, (i * 43) as u8])
        .collect();
    let source = batch.texture_rgba8(extent, pixels)?;
    let tiles = [3, 0];
    for kind in 0..3 {
        for output in 0..2 {
            for scale in [0.0, 1.0, 12.0, -2.0] {
                for compact in [false, true] {
                    for cone in [-1.0, 20.0, 90.0] {
                        let c = FilterConfig {
                            width: extent[0],
                            height: extent[1],
                            region_x0: 1,
                            region_y0: 2,
                            region_width: 15,
                            region_height: 16,
                            dispatch_width: 2,
                            light_kind: kind,
                            lighting_output_kind: output,
                            surface_scale: scale,
                            light_constant: 1.25,
                            specular_exponent: 8.0,
                            light_r: 0.8,
                            light_g: 0.45,
                            light_b: 0.15,
                            surface_origin_x: -7,
                            surface_origin_y: 3,
                            light_p0: 33.0,
                            light_p1: 47.0,
                            light_p2: 15.0,
                            light_p3: 8.0,
                            light_p4: 12.0,
                            light_p5: -3.0,
                            light_p6: 3.5,
                            light_p7: cone,
                            ..Default::default()
                        };
                        let target = batch.texture_rgba8(extent, vec![57; 17 * 19 * 4])?;
                        lighting::encode(
                            &mut batch,
                            c,
                            compact.then_some(tiles.as_slice()),
                            source,
                            target,
                        )?;
                        batch.readback(target)?;
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
                "lighting mode corpus",
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
fn four_api_lighting_plane_normal_and_degenerate_directions() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut expected = Vec::new();
    // The alpha plane has the same x/y slope at the center and one-sided edges.
    let pixels: Vec<u8> = (0..3)
        .flat_map(|y| (0..3).flat_map(move |x| [0, 0, 0, (20 + x * 20 + y * 30) as u8]))
        .collect();
    let source = batch.texture_rgba8([3, 3], pixels)?;
    let norm = (1.0f64 + (40.0f64 / 255.0).powi(2) + (60.0f64 / 255.0).powi(2)).sqrt();
    for output in [0, 1] {
        let c = FilterConfig {
            width: 3,
            height: 3,
            region_width: 3,
            region_height: 3,
            surface_scale: 1.0,
            lighting_output_kind: output,
            light_p1: 90.0,
            light_constant: 1.0,
            specular_exponent: 2.0,
            light_r: 1.0,
            light_g: 0.5,
            light_b: 0.25,
            ..Default::default()
        };
        let target = batch.texture_rgba8([3, 3], vec![57; 36])?;
        lighting::encode(&mut batch, c, None, source, target)?;
        batch.readback(target)?;
        let amount = if output == 0 {
            1.0 / norm
        } else {
            1.0 / (norm * norm)
        };
        let rgba = [
            (255.0 * amount).round() as u8,
            (127.5 * amount).round() as u8,
            (63.75 * amount).round() as u8,
            if output == 0 {
                255
            } else {
                (255.0 * amount).round() as u8
            },
        ];
        expected.push(rgba.repeat(9));
    }
    let flat = batch.texture_rgba8([1, 1], vec![0; 4])?;
    for output in [0, 1] {
        for scenario in 0..4 {
            let mut c = FilterConfig {
                width: 1,
                height: 1,
                region_width: 1,
                region_height: 1,
                lighting_output_kind: output,
                light_kind: 1,
                light_p0: 0.5,
                light_p1: 0.5,
                light_constant: 1.0,
                specular_exponent: 1.0,
                light_r: 0.5,
                light_g: 0.25,
                light_b: 0.125,
                ..Default::default()
            };
            // Coincident point, zero spot direction, upward spot direction, and a valid downward spot.
            if scenario > 0 {
                c.light_kind = 2;
                c.light_p2 = 1.0;
                c.light_p3 = 0.5;
                c.light_p4 = 0.5;
                c.light_p5 = match scenario {
                    1 => 1.0,
                    2 => 2.0,
                    _ => 0.0,
                };
                c.light_p6 = 2.0;
                c.light_p7 = 45.0;
            }
            let target = batch.texture_rgba8([1, 1], vec![57; 4])?;
            lighting::encode(&mut batch, c, None, flat, target)?;
            batch.readback(target)?;
            expected.push(if scenario == 3 {
                vec![128, 64, 32, if output == 0 { 255 } else { 128 }]
            } else {
                vec![0, 0, 0, if output == 0 { 255 } else { 0 }]
            });
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "lighting independent plane and direction oracles",
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
fn four_api_lighting_zero_exponent_and_opposite_half_vector() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut expected = Vec::new();
    let plane = batch.texture_rgba8([2, 1], vec![0, 0, 0, 0, 0, 0, 0, 255])?;
    // At pixel0 N=(-1,0,1), H=(1,0,1): N.H is exactly zero.
    // At pixel1 the light points down, so its half-vector is exactly zero.
    for exponent in [0.0, -3.0] {
        let c = FilterConfig {
            width: 2,
            height: 1,
            region_width: 2,
            region_height: 1,
            surface_scale: 0.5,
            light_kind: 1,
            lighting_output_kind: 1,
            light_p0: 1.5,
            light_p1: 0.5,
            light_constant: 1.0,
            specular_exponent: exponent,
            light_r: 0.5,
            light_g: 0.25,
            light_b: 0.125,
            ..Default::default()
        };
        let target = batch.texture_rgba8([2, 1], vec![57; 8])?;
        lighting::encode(&mut batch, c, None, plane, target)?;
        batch.readback(target)?;
        expected.push(vec![128, 64, 32, 128, 0, 0, 0, 0]);
    }
    let flat = batch.texture_rgba8([1, 1], vec![0; 4])?;
    for output in [0, 1] {
        for exponent in [0.0, -3.0] {
            for back_facing in [false, true] {
                let c = FilterConfig {
                    width: 1,
                    height: 1,
                    region_width: 1,
                    region_height: 1,
                    light_kind: 2,
                    lighting_output_kind: output,
                    light_p0: 0.5,
                    light_p1: 0.5,
                    light_p2: 1.0,
                    light_p3: if back_facing { 0.5 } else { 1.5 },
                    light_p4: 0.5,
                    light_p5: if back_facing { 2.0 } else { 1.0 },
                    light_p6: exponent,
                    light_p7: -1.0,
                    light_constant: 1.0,
                    specular_exponent: 1.0,
                    light_r: 0.5,
                    light_g: 0.25,
                    light_b: 0.125,
                    ..Default::default()
                };
                let target = batch.texture_rgba8([1, 1], vec![57; 4])?;
                lighting::encode(&mut batch, c, None, flat, target)?;
                batch.readback(target)?;
                // SVG requires L.S > 0 to be dark regardless of exponent. At L.S=0,
                // the zero-exponent attenuation is one and no limiting cone applies.
                expected.push(if back_facing {
                    vec![0, 0, 0, if output == 0 { 255 } else { 0 }]
                } else {
                    vec![128, 64, 32, if output == 0 { 255 } else { 128 }]
                });
            }
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "zero lighting exponent and opposite half vector",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
