use super::four_api::Routes;
use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, SamplerFilter},
};
use crate::shared::{
    fine_config::FineConfig,
    gpu_constants::{
        BRUSH_TEXTURE_PLACEMENT_BIT, FINE_WORKGROUP_SIZE, NATIVE_TEXTURE_TABLE_CAPACITY,
    },
};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_unified_brush_dispatches_each_kind_and_resource_placement() -> Result<()> {
    let colors = [
        [17u8, 31, 47, 127],
        [83, 61, 29, 127],
        [31, 73, 97, 127],
        [109, 23, 53, 127],
    ];
    let texel = [29, 43, 71, 255];
    let mut paint = vec![0x12345678u32; 7];
    let mut requests = Vec::new();
    let mut expected = Vec::new();
    for kind in 1..=7u32 {
        for placement in [
            0,
            BRUSH_TEXTURE_PLACEMENT_BIT | (NATIVE_TEXTURE_TABLE_CAPACITY - 1),
        ] {
            let offset = (paint.len() - 7) as u32;
            let pattern = kind == 7;
            paint.extend([
                kind,
                0,
                if pattern { 0 } else { 21 },
                if pattern {
                    0
                } else if kind == 5 {
                    4
                } else {
                    2
                },
                if pattern {
                    placement
                } else {
                    u32::from_le_bytes(colors[0])
                },
                1,
                1,
                255,
                0,
            ]);
            let mut params = [0.0f32; 12];
            match kind {
                2 => {
                    params[2] = 1.0;
                    params[4] = 1.0;
                    params[7] = 1.0;
                }
                3 => {
                    params[5] = 1.0;
                    params[6] = 1.0;
                    params[9] = 1.0;
                }
                4 => {
                    params[3] = std::f32::consts::FRAC_PI_2;
                }
                5 => {
                    params[2] = 1.0;
                    params[3] = 1.0;
                }
                _ => {}
            }
            paint.extend(params.map(f32::to_bits));
            paint.extend(colors.map(u32::from_le_bytes));
            // Endpoint/corner colors distinguish each dispatch branch independently of shader math.
            for (point, (x, y)) in [(0.0f32, 0.0f32), (1.0, 0.0), (0.0, 1.0)]
                .into_iter()
                .enumerate()
            {
                requests.extend([offset, x.to_bits(), y.to_bits(), 0]);
                expected.extend(match kind {
                    2 => colors[usize::from(point == 1)],
                    3 => colors[usize::from(point != 0)],
                    4 => colors[usize::from(point == 2)],
                    5 => colors[[0, 1, 3][point]],
                    6 => [0; 4],
                    7 => texel,
                    _ => colors[0],
                });
            }
        }
    }
    check_brushes(
        paint,
        requests,
        expected,
        [1, 1],
        vec![texel.to_vec(); NATIVE_TEXTURE_TABLE_CAPACITY as usize],
    )
}

fn check_brushes(
    paint: Vec<u32>,
    requests: Vec<u32>,
    mut expected: Vec<u8>,
    size: [u32; 2],
    images: Vec<Vec<u8>>,
) -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let count = requests.len() / 4;
    let config = batch.buffer(
        bytemuck::bytes_of(&FineConfig {
            paint_brush_base: 7,
            ..Default::default()
        })
        .to_vec(),
    )?;
    let paint = batch.buffer(paint.into_iter().flat_map(u32::to_le_bytes).collect())?;
    let requests = batch.buffer(requests.into_iter().flat_map(u32::to_le_bytes).collect())?;
    let request_config = batch.buffer(
        [count as u32, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
    )?;
    let output = batch.buffer(vec![0x37; (count + 5) * 4])?;
    let atlas = batch.texture_array_rgba8([size[0], size[1], 1], images[0].clone())?;
    let members = images
        .into_iter()
        .map(|pixels| batch.texture_rgba8(size, pixels))
        .collect::<Result<Vec<_>>>()?;
    let images = batch.texture_table(&members)?;
    let sampler = batch.sampler(SamplerFilter::Linear)?;
    // SAFETY: complete brush records, valid table indices and atlas rectangles;
    // logical request count excludes the poisoned output tail.
    unsafe {
        batch.dispatch(
            "brush_words",
            &[
                (0, config),
                (3, paint),
                (9, requests),
                (10, output),
                (11, request_config),
                (12, atlas),
                (13, sampler),
                (30, images),
            ],
            [(count as u32).div_ceil(FINE_WORKGROUP_SIZE) + 1, 1, 1],
        )?;
    }
    batch.readback(output)?;
    expected.extend([0x37; 20]);
    routes.check(
        &batch,
        &[expected],
        "all brush kinds and atlas/table placement",
    )?;
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_texture_brush_sampling_matches_independent_channel_oracle() -> Result<()> {
    for size in [[4u32, 4u32], [2, 2], [3, 5], [1, 5], [5, 1]] {
        let images: Vec<Vec<u8>> = (0..NATIVE_TEXTURE_TABLE_CAPACITY)
            .map(|index| {
                (0..size[0] * size[1])
                    .flat_map(|pixel| {
                        if size == [4, 4] {
                            [
                                (pixel % 4 * 32) as u8,
                                (pixel / 4 * 32) as u8,
                                (index * 4) as u8,
                                255,
                            ]
                        } else {
                            let a = (pixel * 31 + index * 47) % 256;
                            [
                                (pixel * 17 + index * 13).rem_euclid(a + 1) as u8,
                                (pixel * 29 + index * 7).rem_euclid(a + 1) as u8,
                                (pixel * 43 + index * 23).rem_euclid(a + 1) as u8,
                                a as u8,
                            ]
                        }
                    })
                    .collect()
            })
            .collect();
        let mut paint = vec![0x12345678u32; 7];
        let mut requests = Vec::new();
        let mut expected = Vec::new();
        // Odd dimensions use centers, where small normalized-coordinate error cannot
        // change the independent byte oracle. Power-of-two images include half-channel ties.
        let positions: &[f32] = if size[0].is_power_of_two() && size[1].is_power_of_two() {
            &[-1.25, -0.5, 0.0, 0.25, 0.5, 1.0, 1.25, 3.5, 4.25]
        } else {
            &[-1.5, -0.5, 0.5, 1.5, 2.5, 3.5, 5.5]
        };
        for index in 0..NATIVE_TEXTURE_TABLE_CAPACITY {
            for extend in 0..3u32 {
                for sampling in 0..2u32 {
                    for opacity in [0, 127, 255u32] {
                        let offset = (paint.len() - 7) as u32;
                        paint.extend([
                            7,
                            extend,
                            0,
                            0,
                            BRUSH_TEXTURE_PLACEMENT_BIT | index,
                            size[0],
                            size[1],
                            opacity,
                            sampling,
                        ]);
                        paint.extend(
                            [
                                1.0 / size[0] as f32,
                                0.0,
                                0.0,
                                1.0 / size[1] as f32,
                                0.0,
                                0.0,
                                0.0,
                                0.0,
                                0.0,
                                0.0,
                                0.0,
                                0.0,
                            ]
                            .map(f32::to_bits),
                        );
                        let coord = |v: i32, n: u32| match extend {
                            1 => v.rem_euclid(n as i32),
                            2 => {
                                let q = v.rem_euclid(2 * n as i32);
                                if q < n as i32 {
                                    q
                                } else {
                                    2 * n as i32 - 1 - q
                                }
                            }
                            _ => v.clamp(0, n as i32 - 1),
                        };
                        for &x in positions {
                            for &y in positions {
                                requests.extend([offset, x.to_bits(), y.to_bits(), 0]);
                                for channel in 0..4 {
                                    let pixel = |x: i32, y: i32| {
                                        f32::from(
                                            images[index as usize][((coord(y, size[1])
                                                * size[0] as i32
                                                + coord(x, size[0]))
                                                * 4)
                                                as usize
                                                + channel],
                                        )
                                    };
                                    let value = if sampling == 0 {
                                        pixel(x.floor() as i32, y.floor() as i32)
                                    } else {
                                        let sx = x - 0.5;
                                        let sy = y - 0.5;
                                        let ix = sx.floor() as i32;
                                        let iy = sy.floor() as i32;
                                        let fx = sx - sx.floor();
                                        let fy = sy - sy.floor();
                                        let row = |y| {
                                            let a = pixel(ix, y);
                                            let b = pixel(ix + 1, y);
                                            let value = a + (b - a) * fx;
                                            if extend == 0 {
                                                value
                                            } else {
                                                (value + 0.5).floor()
                                            }
                                        };
                                        let top = row(iy);
                                        (top + (row(iy + 1) - top) * fy + 0.5).floor()
                                    };
                                    expected.push(((value as u32 * opacity + 127) / 255) as u8);
                                }
                            }
                        }
                    }
                }
            }
        }
        check_brushes(paint, requests, expected, size, images)?;
    }
    Ok(())
}
