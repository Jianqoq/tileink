use super::super::compute::{ComputeBatch, SamplerFilter};
use super::four_api::Routes;
use crate::native::runtime::Result;
use crate::shared::{fine_config::FineConfig, gpu_constants::FINE_WORKGROUP_SIZE};

struct PatternCase {
    page: u32,
    extend: u32,
    sampling: u32,
    opacity: u32,
    size: [u32; 2],
    transform: [f32; 6],
}
impl PatternCase {
    fn add(&self, paint: &mut Vec<u32>) -> u32 {
        let offset = paint.len() as u32;
        paint.extend([
            7,
            self.extend,
            2,
            1,
            self.page,
            self.size[0],
            self.size[1],
            self.opacity,
            self.sampling,
        ]);
        paint.extend(
            self.transform
                .iter()
                .enumerate()
                .map(|(i, v)| (v / self.size[i % 2].max(1) as f32).to_bits()),
        );
        paint.extend([0; 6]);
        offset
    }
    fn expected(&self, atlas: &[u8], x: f32, y: f32) -> [u8; 4] {
        if self.size.contains(&0) {
            return [0; 4];
        }
        let [a, b, c, d, e, f] = self.transform;
        let tx = a * x + c * y + e;
        let ty = b * x + d * y + f;
        let coordinate = |v: i32, size: u32| match self.extend {
            1 => v.rem_euclid(size as i32),
            2 => {
                let q = v.rem_euclid(2 * size as i32);
                if q < size as i32 {
                    q
                } else {
                    2 * size as i32 - q - 1
                }
            }
            _ => v.clamp(0, size as i32 - 1),
        };
        let pixel = |px: i32, py: i32, channel: usize, local: bool| {
            let px = if local {
                coordinate(px, self.size[0]) + 2
            } else {
                px.clamp(0, 7)
            };
            let py = if local {
                coordinate(py, self.size[1]) + 1
            } else {
                py.clamp(0, 7)
            };
            atlas[((self.page * 64 + py as u32 * 8 + px as u32) * 4) as usize + channel] as f32
        };
        std::array::from_fn(|channel| {
            let value = if self.sampling == 0 {
                pixel(tx.floor() as i32, ty.floor() as i32, channel, true)
            } else {
                let (sx, sy, local) = if self.extend == 0 {
                    (
                        tx.clamp(0.0, self.size[0] as f32) + 1.5,
                        ty.clamp(0.0, self.size[1] as f32) + 0.5,
                        false,
                    )
                } else {
                    (tx - 0.5, ty - 0.5, true)
                };
                let ix = sx.floor() as i32;
                let iy = sy.floor() as i32;
                let fx = sx - sx.floor();
                let fy = sy - sy.floor();
                let row = |y| {
                    let left = pixel(ix, y, channel, local);
                    let right = pixel(ix + 1, y, channel, local);
                    let value = left + (right - left) * fx;
                    if local { (value + 0.5).floor() } else { value }
                };
                let top = row(iy);
                let bottom = row(iy + 1);
                (top + (bottom - top) * fy + 0.5).floor()
            };
            ((value as u32 * self.opacity + 127) / 255) as u8
        })
    }
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_atlas_patterns_match_production_sampling_and_cpu_bytes() -> Result<()> {
    let atlas: Vec<u8> = (0..3 * 64 * 4)
        .map(|i| (((i * 17 + i / 32 * 7) % 64) * 4) as u8)
        .collect();
    let coordinates = [
        -4.0f32, -1.25, -1.0, -0.5, 0.0, 0.25, 0.5, 1.0, 1.5, 2.0, 3.5, 4.0, 5.0,
    ];
    let mut paint = Vec::new();
    let mut requests = Vec::new();
    let mut expected = Vec::new();
    for page in 0..3 {
        for extend in 0..3 {
            for sampling in 0..2 {
                for opacity in [0, 127, 255] {
                    for size in [[4, 4], [1, 1], [0, 4], [3, 5]] {
                        for transform in [
                            [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                            [0.0, -1.0, 1.0, 0.0, 2.0, 3.0],
                            [1.0, 0.0, 0.0, 1.0, -0.5, 0.25],
                        ] {
                            // Keep the independent non-power-of-two oracle at texel centers;
                            // normalized translation coefficients can intentionally round across integer boundaries.
                            if size == [3, 5] && transform[4] == -0.5 {
                                continue;
                            }
                            let case = PatternCase {
                                page,
                                extend,
                                sampling,
                                opacity,
                                size,
                                transform,
                            };
                            let offset = case.add(&mut paint);
                            let points: &[f32] = if size == [3, 5] {
                                &[-0.5, 0.0, 0.5, 1.5, 2.5, 3.5, 4.5, 5.5]
                            } else {
                                &coordinates
                            };
                            for &x in points {
                                for &y in points {
                                    requests.extend([offset, x.to_bits(), y.to_bits(), 0]);
                                    expected.extend(case.expected(&atlas, x, y));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    check_patterns(atlas, paint, requests, expected)
}
fn check_patterns(
    atlas: Vec<u8>,
    paint: Vec<u32>,
    requests: Vec<u32>,
    mut expected: Vec<u8>,
) -> Result<()> {
    let count = requests.len() / 4;
    let config = FineConfig {
        paint_brush_base: 7,
        ..Default::default()
    };
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(bytemuck::bytes_of(&config).to_vec())?;
    let paint = batch.buffer(
        [0x12345678u32; 7]
            .into_iter()
            .chain(paint)
            .flat_map(u32::to_le_bytes)
            .collect(),
    )?;
    let requests = batch.buffer(requests.into_iter().flat_map(u32::to_le_bytes).collect())?;
    let output = batch.buffer(vec![0x37; (count + 5) * 4])?;
    let request_config = batch.buffer(
        [count as u32, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
    )?;
    let atlas = batch.texture_array_rgba8([8, 8, 3], atlas)?;
    let sampler = batch.sampler(SamplerFilter::Linear)?;
    // SAFETY: complete serialized brush records, valid atlas rectangles/pages, and guarded output lanes.
    unsafe {
        batch.dispatch(
            "pattern_words",
            &[
                (0, config),
                (3, paint),
                (9, requests),
                (10, output),
                (11, request_config),
                (12, atlas),
                (13, sampler),
            ],
            [(count as u32).div_ceil(FINE_WORKGROUP_SIZE) + 1, 1, 1],
        )?;
    }
    batch.readback(output)?;
    expected.extend([0x37; 20]);
    let routes = Routes::new()?;
    routes.check(
        &batch,
        &[expected],
        "production atlas patterns with explicit resources",
    )?;
    eprintln!("M4 atlas pattern cases: {count}");
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_atlas_negative_repeat_uses_euclidean_coordinates() -> Result<()> {
    let atlas: Vec<u8> = (0..3 * 64 * 4)
        .map(|i| (((i * 17 + i / 32 * 7) % 64) * 4) as u8)
        .collect();
    let case = PatternCase {
        page: 0,
        extend: 1,
        sampling: 0,
        opacity: 127,
        size: [3, 5],
        transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };
    let mut paint = Vec::new();
    let offset = case.add(&mut paint);
    let coordinate = -0.5f32;
    let expected = case.expected(&atlas, coordinate, coordinate).to_vec();
    check_patterns(
        atlas,
        paint,
        vec![offset, coordinate.to_bits(), coordinate.to_bits(), 0],
        expected,
    )
}
