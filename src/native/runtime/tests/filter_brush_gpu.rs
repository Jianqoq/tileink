use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, SamplerFilter},
};
use crate::shared::{
    filter_config::FilterConfig,
    gpu_constants::{
        BRUSH_TEXTURE_PLACEMENT_BIT, FILTER_WORKGROUP_SIZE, NATIVE_TEXTURE_TABLE_CAPACITY,
        TILE_SIZE,
    },
};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_brush_filters_preserve_region_tiles_padding_and_integer_composition() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    for table_enabled in [false, true] {
        let mut batch = ComputeBatch::new();
        let color = [17u8, 31, 47, 127];
        let atlas = batch.texture_array_rgba8([1, 1, 1], color.to_vec())?;
        let image = batch.texture_rgba8([1, 1], color.to_vec())?;
        let images = batch.texture_table(&vec![image; NATIVE_TEXTURE_TABLE_CAPACITY as usize])?;
        let sampler = batch.sampler(SamplerFilter::Linear)?;
        let mut expected = Vec::new();
        for pattern in [false, true] {
            let mut paint = vec![0x12345678u32; 7];
            paint.extend([
                if pattern { 7 } else { 1 },
                0,
                0,
                0,
                if pattern {
                    if table_enabled {
                        BRUSH_TEXTURE_PLACEMENT_BIT | (NATIVE_TEXTURE_TABLE_CAPACITY - 1)
                    } else {
                        0
                    }
                } else {
                    u32::from_le_bytes(color)
                },
                1,
                1,
                255,
                0,
            ]);
            paint.extend([0; 12]);
            let paint = batch.buffer(paint.into_iter().flat_map(u32::to_le_bytes).collect())?;
            for compact in [false, true] {
                for shadow in [false, true] {
                    let tiles = [5u32, 0, 2];
                    let c = FilterConfig {
                        width: 33,
                        height: 19,
                        tiles_width: 3,
                        tiles_height: 2,
                        region_x0: 1,
                        region_y0: 1,
                        region_width: 32,
                        region_height: 18,
                        compact_tiles: u32::from(compact),
                        active_tile_count: if compact { 3 } else { 0 },
                        pixel_count: if compact {
                            3 * TILE_SIZE * TILE_SIZE
                        } else {
                            32 * 18
                        },
                        dispatch_width: 2,
                        brush_offset: 7,
                        ..Default::default()
                    };
                    let mut cpu = vec![0x37; 35 * 21 * 4];
                    let target = batch.texture_rgba8([35, 21], cpu.clone())?;
                    let aux_pixels: Vec<_> = (0..35 * 21u32)
                        .flat_map(|i| [0, 0, 0, (i % 256) as u8])
                        .collect();
                    let aux = batch.texture_rgba8([35, 21], aux_pixels.clone())?;
                    let uniform = batch.buffer(bytemuck::bytes_of(&c).to_vec())?;
                    let active =
                        batch.buffer(tiles.into_iter().flat_map(u32::to_le_bytes).collect())?;
                    let mut bindings = vec![
                        (0, uniform),
                        (3, target),
                        (8, active),
                        (10, paint),
                        (12, atlas),
                        (13, sampler),
                        (30, images),
                    ];
                    if shadow {
                        bindings.push((2, aux));
                    }
                    // SAFETY: complete constant brush/image records, distinct owned textures,
                    // unique active tiles, valid region and guarded padded dispatch groups.
                    unsafe {
                        batch.dispatch(
                            if shadow {
                                "filter_composite_drop_shadow_region"
                            } else {
                                "filter_flood_region"
                            },
                            &bindings,
                            [
                                2,
                                c.pixel_count.div_ceil(FILTER_WORKGROUP_SIZE).div_ceil(2),
                                1,
                            ],
                        )?;
                    }
                    for y in 1..19u32 {
                        for x in 1..33u32 {
                            if compact && !tiles.contains(&(y / TILE_SIZE * 3 + x / TILE_SIZE)) {
                                continue;
                            }
                            let i = ((y * 35 + x) * 4) as usize;
                            if shadow {
                                let alpha = u32::from(aux_pixels[i + 3]);
                                let scaled = color.map(|v| (u32::from(v) * alpha + 127) / 255);
                                // Existing foreground is composed over its shadow.
                                for lane in 0..4 {
                                    cpu[i + lane] =
                                        (0x37 + (scaled[lane] * (255 - 0x37) + 127) / 255) as u8;
                                }
                            } else {
                                cpu[i..i + 4].copy_from_slice(&color);
                            }
                        }
                    }
                    batch.readback(target)?;
                    expected.push(cpu);
                }
            }
        }
        for portable in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "brush filters independent integer pixels",
                Some(FilterVariant {
                    portable,
                    texture_table: table_enabled,
                }),
            )?;
        }
    }
    routes.validate()
}
