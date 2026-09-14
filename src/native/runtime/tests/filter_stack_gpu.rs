use super::{filter_layer::path_draw, four_api::Routes, reference::FilterVariant};
use crate::{
    native::runtime::{
        Result,
        compute::ComputeBatch,
        program::filter::{
            layer::{Geometry, Scene},
            stack::{Composite, Stack, Textures},
        },
    },
    shared::{
        filter_config::FilterConfig, gpu_coarse::LayerStackRecord, path::PathRecord,
        tile_seg_range::TileSegmentRange,
    },
};
use peniko::Compose;

fn scale(pixel: [u8; 4], alpha: u32) -> [u8; 4] {
    pixel.map(|v| ((u32::from(v) * alpha + 127) / 255) as u8)
}
fn over(destination: [u8; 4], source: [u8; 4]) -> [u8; 4] {
    std::array::from_fn(|i| {
        (u32::from(source[i])
            + (u32::from(destination[i]) * (255 - u32::from(source[3])) + 127) / 255) as u8
    })
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_stack_composites_match_integer_oracles_and_capacity_boundary() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let geometry = Geometry::upload(
        &mut batch,
        Scene {
            draws: &[path_draw(0)],
            paths: &[PathRecord {
                tile_x1: 2,
                tile_y1: 2,
                data_len: 4,
                ..Default::default()
            }],
            backdrops: &[1; 4],
            ranges: &[TileSegmentRange { start: 0, end: 0 }; 4],
            ..Default::default()
        },
    )?;
    let opacity = |payload| LayerStackRecord {
        tag: 1,
        draw: 0,
        payload,
    };
    let blocked = LayerStackRecord {
        tag: 0,
        draw: u32::MAX,
        payload: 0,
    };
    // The final zero-opacity group must be ignored beyond the defined shader stack capacity;
    // a clip after that ignored group still applies. These are semantic boundary fixtures.
    let mut overflow =
        vec![opacity(255); crate::shared::gpu_constants::FILTER_GROUP_STACK_CAPACITY as usize];
    overflow.push(opacity(0));
    let mut overflow_clip = overflow.clone();
    overflow_clip.push(blocked);
    let cases = vec![
        (vec![], vec![], false),
        (vec![blocked], vec![], true),
        (vec![opacity(128)], vec![128], false),
        (vec![opacity(128), opacity(128)], vec![128, 128], false),
        (
            vec![LayerStackRecord {
                tag: 2,
                draw: 0,
                payload: (Compose::SrcOver as u32) << 8,
            }],
            vec![],
            false,
        ),
        (overflow, vec![], false),
        (overflow_clip, vec![], true),
    ];
    let foreground = [91u8, 47, 17, 127];
    let initial = [13u8, 37, 81, 255];
    let source = batch.texture_rgba8([32, 32], foreground.repeat(32 * 32))?;
    let small = batch.texture_rgba8([7, 5], foreground.repeat(7 * 5))?;
    let auxiliary = batch.texture_rgba8([32, 32], [0u8, 0, 0, 173].repeat(32 * 32))?;
    let mut expected = Vec::new();
    for (layers, opacities, is_blocked) in cases {
        let end = 1 + layers.len() as u32;
        // Poison both sides so the shader must respect both bounds of the selected range.
        let mut padded = vec![blocked];
        padded.extend(layers);
        padded.push(blocked);
        let stack = Stack::upload(&mut batch, geometry, &padded)?;
        for (mode, surface, masked) in [
            (Composite::Over, false, false),
            (Composite::Over, false, true),
            (Composite::Blend, false, true),
            (Composite::Surface, true, false),
        ] {
            for compact in [false, true] {
                let target = batch.texture_rgba8([34, 34], initial.repeat(34 * 34))?;
                let config = FilterConfig {
                    width: 32,
                    height: 32,
                    region_x0: 1,
                    region_y0: 1,
                    region_width: 31,
                    region_height: 31,
                    layer_stack_start: 1,
                    layer_stack_end: end,
                    mask_enabled: u32::from(!masked),
                    blend_mode: (Compose::SrcOver as u32) << 8,
                    kernel_columns: 7,
                    kernel_rows: 5,
                    offset_x: 3,
                    offset_y: 2,
                    dispatch_width: 2,
                    ..Default::default()
                };
                let mut color = if masked {
                    scale(foreground, 173)
                } else {
                    foreground
                };
                for opacity in &opacities {
                    color = scale(color, *opacity);
                }
                let result = if is_blocked {
                    initial
                } else {
                    over(over(initial, color), color)
                };
                for _ in 0..2 {
                    stack.encode(
                        &mut batch,
                        mode,
                        config,
                        compact.then_some(&[3, 0]),
                        Textures {
                            source: if surface { small } else { source },
                            auxiliary: masked.then_some(auxiliary),
                            target,
                        },
                    )?;
                }
                batch.readback(target)?;
                let mut pixels = initial.repeat(34 * 34);
                for y in 1..32 {
                    for x in 1..32 {
                        let tile = y / 16 * 2 + x / 16;
                        if compact && tile != 0 && tile != 3 {
                            continue;
                        }
                        if surface && (!(3..10).contains(&x) || !(2..7).contains(&y)) {
                            continue;
                        }
                        pixels[(y * 34 + x) * 4..(y * 34 + x) * 4 + 4].copy_from_slice(&result);
                    }
                }
                expected.push(pixels);
            }
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "stack nesting, opacity and capacity independent integer oracle",
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
fn stack_rejects_logical_ranges_aliases_and_foreign_geometry() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let geometry = Geometry::upload(&mut batch, Scene::default())?;
    let stack = Stack::upload(&mut batch, geometry, &[])?;
    let source = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let config = FilterConfig {
        width: 1,
        height: 1,
        region_width: 1,
        region_height: 1,
        ..Default::default()
    };
    let textures = || Textures {
        source,
        auxiliary: Some(source),
        target,
    };
    assert!(
        stack
            .encode(
                &mut batch,
                Composite::Over,
                FilterConfig {
                    layer_stack_end: 1,
                    ..config
                },
                None,
                textures()
            )
            .is_err()
    );
    assert!(
        stack
            .encode(
                &mut batch,
                Composite::Over,
                FilterConfig {
                    layer_stack_start: 1,
                    ..config
                },
                None,
                textures()
            )
            .is_err()
    );
    assert!(
        stack
            .encode(
                &mut batch,
                Composite::Blend,
                config,
                None,
                Textures {
                    source,
                    auxiliary: None,
                    target
                }
            )
            .is_err()
    );
    assert!(
        stack
            .encode(
                &mut batch,
                Composite::Blend,
                config,
                None,
                Textures {
                    source: target,
                    auxiliary: Some(source),
                    target
                }
            )
            .is_err()
    );
    assert!(
        stack
            .encode(
                &mut batch,
                Composite::Surface,
                FilterConfig {
                    offset_x: i32::MIN,
                    ..config
                },
                None,
                textures()
            )
            .is_err()
    );
    let mut foreign = ComputeBatch::new();
    assert!(Stack::upload(&mut foreign, geometry, &[]).is_err());
    assert!(batch.passes().is_empty());
    assert!(foreign.passes().is_empty());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_stack_blends_transformed_sdf_and_translated_surfaces() -> Result<()> {
    use crate::shared::{affine::GpuAffine, gpu_constants::SDF_RECORD_WORDS};
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut draw = path_draw(0);
    draw.path_id = u32::MAX;
    draw.sdf_offset = 0;
    draw.sdf_len = SDF_RECORD_WORDS;
    draw.inverse_transform = GpuAffine {
        a: 1.0,
        b: 0.25,
        c: 0.2,
        d: 1.0,
        e: -1.0,
        f: -2.0,
    };
    let mut paint = vec![1u32];
    paint.extend(
        [
            5.25f32, 4.5, 25.75, 27.5, 2.0, 3.0, 1.0, 4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ]
        .map(f32::to_bits),
    );
    let geometry = Geometry::upload(
        &mut batch,
        Scene {
            draws: &[draw],
            paint: &paint,
            ..Default::default()
        },
    )?;
    let initial = [37u8, 71, 113, 191].repeat(32 * 32);
    let source = batch.texture_rgba8([32, 32], [91u8, 47, 17, 127].repeat(32 * 32))?;
    let auxiliary = batch.texture_rgba8([32, 32], [0u8, 0, 0, 173].repeat(32 * 32))?;
    let modes = (0..16)
        .map(|mix| mix | ((Compose::SrcOver as u32) << 8))
        .chain((0..14).map(|compose| 2 | (compose << 8)));
    for blend_mode in modes {
        let layers = [
            LayerStackRecord {
                tag: 0,
                draw: 0,
                payload: 0,
            },
            LayerStackRecord {
                tag: 1,
                draw: 0,
                payload: 173,
            },
            LayerStackRecord {
                tag: 2,
                draw: 0,
                payload: blend_mode,
            },
        ];
        let stack = Stack::upload(&mut batch, geometry, &layers)?;
        for mode in [Composite::Over, Composite::Blend] {
            let target = batch.texture_rgba8([32, 32], initial.clone())?;
            stack.encode(
                &mut batch,
                mode,
                FilterConfig {
                    width: 32,
                    height: 32,
                    region_width: 32,
                    region_height: 32,
                    layer_stack_end: 3,
                    blend_mode,
                    mask_enabled: 0,
                    ..Default::default()
                },
                None,
                Textures {
                    source,
                    auxiliary: Some(auxiliary),
                    target,
                },
            )?;
            batch.readback(target)?;
        }
    }
    let stack = Stack::upload(
        &mut batch,
        geometry,
        &[LayerStackRecord {
            tag: 0,
            draw: 0,
            payload: 0,
        }],
    )?;
    for offset in [[-1, 6], [9, -1]] {
        let target = batch.texture_rgba8([32, 32], initial.clone())?;
        stack.encode(
            &mut batch,
            Composite::Surface,
            FilterConfig {
                width: 32,
                height: 32,
                region_width: 32,
                region_height: 32,
                kernel_columns: 16,
                kernel_rows: 16,
                offset_x: offset[0],
                offset_y: offset[1],
                layer_stack_end: 1,
                mask_enabled: 1,
                ..Default::default()
            },
            None,
            Textures {
                source,
                auxiliary: None,
                target,
            },
        )?;
        batch.readback(target)?;
    }
    let expected = routes.filter_reference_output(
        &batch,
        FilterVariant {
            portable: false,
            texture_table: false,
        },
    )?;
    assert!(
        expected.iter().any(|image| image != &initial),
        "corpus must exercise visible composition"
    );
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "stack blend modes, transformed clip and signed surface origins",
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
fn stack_host_rejects_opacity_above_byte_range() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let geometry = Geometry::upload(&mut batch, Scene::default())?;
    assert!(
        Stack::upload(
            &mut batch,
            geometry,
            &[LayerStackRecord {
                tag: crate::shared::gpu_types::GPU_LAYER_OPACITY,
                draw: 0,
                payload: u32::from(u8::MAX) + 1
            }]
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn stack_host_records_over_without_an_auxiliary_texture() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let geometry = Geometry::upload(&mut batch, Scene::default())?;
    let stack = Stack::upload(&mut batch, geometry, &[])?;
    let source = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    stack.encode(
        &mut batch,
        Composite::Over,
        FilterConfig {
            width: 1,
            height: 1,
            region_width: 1,
            region_height: 1,
            ..Default::default()
        },
        None,
        Textures {
            source,
            auxiliary: None,
            target,
        },
    )?;
    assert_eq!(batch.passes().len(), 1);
    Ok(())
}
