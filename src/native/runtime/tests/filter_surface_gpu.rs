use super::{four_api::Routes, reference::FilterVariant};
use crate::{
    native::runtime::{Result, compute::ComputeBatch, program::filter::surface},
    shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE},
};

#[test]
fn surface_validates_source_domain_translation_and_ownership() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([2, 3], vec![0; 24])?;
    let target = batch.texture_rgba8([4, 5], vec![0; 80])?;
    let c = FilterConfig {
        width: 4,
        height: 5,
        region_width: 4,
        region_height: 5,
        kernel_columns: 2,
        kernel_rows: 3,
        ..Default::default()
    };
    for bad in [
        FilterConfig {
            kernel_columns: 3,
            ..c
        },
        FilterConfig {
            kernel_rows: 4,
            ..c
        },
        FilterConfig {
            offset_x: i32::MIN,
            ..c
        },
        FilterConfig {
            offset_y: i32::MAX,
            ..c
        },
    ] {
        assert!(surface::encode(&mut batch, bad, None, source, target).is_err());
    }
    let mut other = ComputeBatch::new();
    let foreign = other.texture_rgba8([2, 3], vec![0; 24])?;
    assert!(surface::encode(&mut batch, c, None, foreign, target).is_err());
    assert!(surface::encode(&mut batch, c, None, target, target).is_err());
    assert!(batch.passes().is_empty());
    surface::encode(&mut batch, c, None, source, target)?;
    assert_eq!(batch.passes().len(), 1);
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_surface_translated_different_extents_match_integer_oracle() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let size = [33u32, 19];
    let source_size = [7u32, 5];
    let allocation = [9u32, 7];
    let target_allocation = [35u32, 21];
    let pixels: Vec<u8> = (0..63u32)
        .flat_map(|i| {
            if i % 9 >= 7 || i / 9 >= 5 {
                return [200, 0, 200, 200];
            }
            let a = i * 47 % 256;
            let d = a + 1;
            [
                (i * 11 % d) as u8,
                (i * 37 % d) as u8,
                (i * 71 % d) as u8,
                a as u8,
            ]
        })
        .collect();
    let source = batch.texture_rgba8(allocation, pixels.clone())?;
    let initial: Vec<u8> = (0..target_allocation[0] * target_allocation[1])
        .flat_map(|i| {
            if i % target_allocation[0] >= size[0] || i / target_allocation[0] >= size[1] {
                [255u8, 0, 255, 255]
            } else {
                [37, 71, 113, 255]
            }
        })
        .collect();
    let tiles = [5, 0, 2];
    let mut expected = Vec::new();
    for offset in [[0, 0], [3, 2], [-3, -2], [29, 16], [100, -100]] {
        for compact in [false, true] {
            for logical in [source_size, [0, 5], [7, 0]] {
                let c = FilterConfig {
                    width: size[0],
                    height: size[1],
                    region_x0: 1,
                    region_y0: 1,
                    region_width: 32,
                    region_height: 18,
                    kernel_columns: logical[0],
                    kernel_rows: logical[1],
                    offset_x: offset[0],
                    offset_y: offset[1],
                    dispatch_width: 2,
                    ..Default::default()
                };
                let target = batch.texture_rgba8(target_allocation, initial.clone())?;
                let mut cpu = initial.clone();
                for _ in 0..2 {
                    surface::encode(
                        &mut batch,
                        c,
                        compact.then_some(tiles.as_slice()),
                        source,
                        target,
                    )?;
                    for y in 1..size[1] {
                        for x in 1..size[0] {
                            if compact
                                && !tiles.contains(
                                    &(y / TILE_SIZE * size[0].div_ceil(TILE_SIZE) + x / TILE_SIZE),
                                )
                            {
                                continue;
                            }
                            let sx = i64::from(x) - i64::from(offset[0]);
                            let sy = i64::from(y) - i64::from(offset[1]);
                            if sx < 0
                                || sy < 0
                                || sx >= i64::from(logical[0])
                                || sy >= i64::from(logical[1])
                            {
                                continue;
                            }
                            let si = ((sy as u32 * allocation[0] + sx as u32) * 4) as usize;
                            let di = ((y * target_allocation[0] + x) * 4) as usize;
                            let a = u32::from(pixels[si + 3]);
                            for lane in 0..4 {
                                cpu[di + lane] = (u32::from(pixels[si + lane])
                                    + (u32::from(cpu[di + lane]) * (255 - a) + 127) / 255)
                                    as u8;
                            }
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
                "translated surfaces independent integer oracle",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
