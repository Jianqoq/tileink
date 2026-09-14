use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{
    Result,
    compute::ComputeBatch,
    program::filter::layer::{Geometry, Scene},
};
use crate::shared::{
    affine::GpuAffine,
    bounds::PixelBounds,
    draw_record::{DrawRecord, DrawTag, FillRuleWord},
    filter_config::FilterConfig,
    path::PathRecord,
    tile_seg_range::TileSegmentRange,
};
use bytemuck::Zeroable;

pub(super) fn path_draw(fill: u32) -> DrawRecord {
    DrawRecord {
        path_id: 0,
        glyph_run_id: u32::MAX,
        sdf_offset: u32::MAX,
        sdf_shadow_offset: u32::MAX,
        tag: DrawTag::Clip.into(),
        fill_rule: FillRuleWord(fill),
        pixel_bounds: PixelBounds {
            x0: 0,
            y0: 0,
            x1: 32,
            y1: 32,
        },
        transform: GpuAffine::IDENTITY,
        inverse_transform: GpuAffine::IDENTITY,
        ..DrawRecord::zeroed()
    }
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_layer_masks_backdrops_sdf_and_invalid_draw() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut rect = path_draw(0);
    rect.path_id = u32::MAX;
    rect.sdf_offset = 0;
    rect.sdf_len = crate::shared::gpu_constants::SDF_RECORD_WORDS;
    let mut paint = vec![1u32];
    paint.extend([2.0f32, 3.0, 29.0, 26.0].map(f32::to_bits));
    paint.extend([0; 12]);
    let paths = [PathRecord {
        tile_x1: 2,
        tile_y1: 2,
        data_len: 4,
        ..Default::default()
    }];
    let ranges = [TileSegmentRange { start: 0, end: 0 }; 4];
    let geometry = Geometry::upload(
        &mut batch,
        Scene {
            draws: &[path_draw(0), path_draw(1), rect],
            paths: &paths,
            backdrops: &[0, 1, 2, -1],
            ranges: &ranges,
            segments: &[],
            paint: &paint,
            shadow_base: 0,
        },
    )?;
    let initial = vec![19u8; 34 * 34 * 4];
    let mut expected = Vec::new();
    for draw_ix in [0, 1, 2, u32::MAX] {
        for compact in [false, true] {
            let target = batch.texture_rgba8([34, 34], initial.clone())?;
            let config = FilterConfig {
                width: 32,
                height: 32,
                region_x0: 1,
                region_y0: 1,
                region_width: 31,
                region_height: 31,
                draw_ix,
                ..Default::default()
            };
            geometry.mask(&mut batch, config, compact.then_some(&[3, 0]), target)?;
            batch.readback(target)?;
            let mut pixels = initial.clone();
            for y in 1..32usize {
                for x in 1..32usize {
                    let tile = y / 16 * 2 + x / 16;
                    if compact && tile != 3 && tile != 0 {
                        continue;
                    }
                    let covered = match draw_ix {
                        0 => tile != 0,
                        1 => tile == 1 || tile == 3,
                        2 => (2..29).contains(&x) && (3..26).contains(&y),
                        _ => false,
                    };
                    let alpha = if covered { 255 } else { 0 };
                    pixels[(y * 34 + x) * 4..(y * 34 + x) * 4 + 4].fill(alpha);
                }
            }
            expected.push(pixels);
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "layer masks independent integer oracle",
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
fn layer_geometry_rejects_invalid_raw_addresses_before_recording() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let draw = path_draw(0);
    let mut path = PathRecord {
        tile_x1: 1,
        tile_y1: 1,
        ..Default::default()
    };
    let range = TileSegmentRange { start: 0, end: 0 };
    assert!(
        Geometry::upload(
            &mut batch,
            Scene {
                draws: &[draw],
                paths: &[path],
                backdrops: &[],
                ranges: &[range],
                segments: &[],
                paint: &[],
                shadow_base: 0
            }
        )
        .is_err()
    );
    // Logical path extent must not read allocation padding, even when buffers are large enough.
    assert!(
        Geometry::upload(
            &mut batch,
            Scene {
                draws: &[draw],
                paths: &[path],
                backdrops: &[0],
                ranges: &[range],
                segments: &[],
                paint: &[],
                shadow_base: 0
            }
        )
        .is_err()
    );
    path.data_offset = u32::MAX;
    assert!(
        Geometry::upload(
            &mut batch,
            Scene {
                draws: &[draw],
                paths: &[path],
                backdrops: &[0],
                ranges: &[range],
                segments: &[],
                paint: &[],
                shadow_base: 0
            }
        )
        .is_err()
    );
    assert!(
        Geometry::upload(
            &mut batch,
            Scene {
                draws: &[draw],
                paths: &[],
                backdrops: &[0],
                ranges: &[TileSegmentRange { start: 0, end: 1 }],
                segments: &[],
                paint: &[],
                shadow_base: 0
            }
        )
        .is_err()
    );
    let mut sdf = draw;
    sdf.sdf_offset = 0;
    sdf.sdf_len = crate::shared::gpu_constants::SDF_RECORD_WORDS;
    assert!(
        Geometry::upload(
            &mut batch,
            Scene {
                draws: &[sdf],
                paths: &[],
                backdrops: &[],
                ranges: &[],
                segments: &[],
                paint: &[0; 16],
                shadow_base: 0
            }
        )
        .is_err()
    );
    sdf.sdf_offset = u32::MAX;
    sdf.sdf_shadow_offset = 1;
    sdf.sdf_shadow_len = crate::shared::gpu_constants::SDF_RECORD_WORDS;
    assert!(
        Geometry::upload(
            &mut batch,
            Scene {
                draws: &[sdf],
                paths: &[],
                backdrops: &[],
                ranges: &[],
                segments: &[],
                paint: &[0; 17],
                shadow_base: u32::MAX
            }
        )
        .is_err()
    );
    let empty = Geometry::upload(
        &mut batch,
        Scene {
            draws: &[],
            paths: &[],
            backdrops: &[],
            ranges: &[],
            segments: &[],
            paint: &[],
            shadow_base: 0,
        },
    )?;
    let mut foreign = ComputeBatch::new();
    let target = foreign.texture_rgba8([1, 1], vec![0; 4])?;
    assert!(
        empty
            .mask(
                &mut foreign,
                FilterConfig {
                    width: 1,
                    height: 1,
                    region_width: 1,
                    region_height: 1,
                    ..Default::default()
                },
                None,
                target
            )
            .is_err()
    );
    assert!(batch.passes().is_empty());
    assert!(foreign.passes().is_empty());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_layer_masks_segments_shadow_offsets_and_empty() -> Result<()> {
    use crate::shared::{gpu_constants::SDF_RECORD_WORDS, line_seg::LineSegment};
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut paint = vec![1u32];
    paint.extend(
        [
            2.0f32, 3.0, 29.0, 26.0, 2.0, 1.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ]
        .map(f32::to_bits),
    );
    paint.extend([0; 3]);
    paint.push(7);
    paint.extend(
        [
            2.0f32, 3.0, 29.0, 26.0, 2.0, 1.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.25, -0.75, 2.5, 0.7,
        ]
        .map(f32::to_bits),
    );
    let mut normal = path_draw(0);
    normal.path_id = u32::MAX;
    normal.sdf_offset = 0;
    normal.sdf_len = SDF_RECORD_WORDS;
    let mut shadow = normal;
    shadow.sdf_offset = u32::MAX;
    shadow.sdf_shadow_offset = 3;
    shadow.sdf_shadow_len = SDF_RECORD_WORDS;
    let mut both = normal;
    both.sdf_shadow_offset = 3;
    both.sdf_shadow_len = SDF_RECORD_WORDS;
    let transforms = [
        GpuAffine::IDENTITY,
        GpuAffine {
            a: -1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 32.0,
            f: 0.0,
        },
        GpuAffine {
            a: 0.8,
            b: 0.6,
            c: -0.6,
            d: 0.8,
            e: 3.25,
            f: -2.5,
        },
        GpuAffine {
            a: 1.0,
            b: 0.25,
            c: 0.75,
            d: 1.0,
            e: -2.0,
            f: 1.0,
        },
    ];
    let mut parity_pairs = Vec::new();
    let mut output_index = 0usize;
    for transform in transforms {
        let draws = [normal, shadow, both].map(|mut draw| {
            draw.inverse_transform = transform;
            draw
        });
        let geometry = Geometry::upload(
            &mut batch,
            Scene {
                draws: &draws,
                paint: &paint,
                shadow_base: SDF_RECORD_WORDS,
                ..Default::default()
            },
        )?;
        for draw_ix in 0..3 {
            let target = batch.texture_rgba8([32, 32], vec![0; 32 * 32 * 4])?;
            geometry.mask(
                &mut batch,
                FilterConfig {
                    width: 32,
                    height: 32,
                    region_width: 32,
                    region_height: 32,
                    draw_ix,
                    ..Default::default()
                },
                None,
                target,
            )?;
            batch.readback(target)?;
        }
        parity_pairs.push((output_index, output_index + 2));
        output_index += 3;
    }
    let paths = [PathRecord {
        tile_x1: 2,
        tile_y1: 2,
        data_len: 4,
        ..Default::default()
    }];
    let segments = [LineSegment {
        p0x: 8.25,
        p0y: 16.0,
        p1x: 8.25,
        p1y: 0.0,
        y_edge: 16.0,
    }];
    let geometry = Geometry::upload(
        &mut batch,
        Scene {
            draws: &[path_draw(0)],
            paths: &paths,
            backdrops: &[0; 4],
            ranges: &[TileSegmentRange { start: 0, end: 1 }; 4],
            segments: &segments,
            ..Default::default()
        },
    )?;
    let target = batch.texture_rgba8([32, 32], vec![0; 32 * 32 * 4])?;
    let config = FilterConfig {
        width: 32,
        height: 32,
        region_width: 32,
        region_height: 32,
        ..Default::default()
    };
    geometry.mask(&mut batch, config, None, target)?;
    batch.readback(target)?;
    let empty = Geometry::upload(&mut batch, Scene::default())?;
    let blank = batch.texture_rgba8([32, 32], vec![255; 32 * 32 * 4])?;
    empty.mask(&mut batch, config, None, blank)?;
    batch.readback(blank)?;
    let variant = FilterVariant {
        portable: false,
        texture_table: false,
    };
    let expected = routes.filter_reference_output(&batch, variant)?;
    for (normal, both) in parity_pairs {
        assert_eq!(
            expected[normal], expected[both],
            "primary SDF takes precedence"
        );
    }
    let line_oracle: Vec<u8> = (0..32 * 32)
        .flat_map(|i| {
            [match i % 16 {
                0..=7 => 0,
                8 => 191,
                _ => 255,
            }; 4]
        })
        .collect();
    assert_eq!(
        expected[output_index], line_oracle,
        "vertical edge exact area coverage"
    );
    assert!(
        expected[output_index + 1].iter().all(|v| *v == 0),
        "empty logical scene must ignore binding padding"
    );
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "layer shadows, geometry and empty scene",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
