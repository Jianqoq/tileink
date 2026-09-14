use super::super::compute::{ComputeBatch, SamplerFilter};
use super::four_api::Routes;
use crate::native::runtime::Result;
use crate::shared::gpu_constants::FINE_WORKGROUP_SIZE;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_samplers_preserve_filtering_clamping_and_pass_bindings() -> Result<()> {
    let routes = Routes::new()?;
    let texels: Vec<u8> = (0..3u32)
        .flat_map(|layer| {
            (0..2).flat_map(move |y| {
                (0..2).flat_map(move |x| {
                    [
                        16 + 48 * x + 16 * y + 8 * layer,
                        32 + 16 * x + 64 * y + 4 * layer,
                        8 + 32 * x + 24 * y + 12 * layer,
                        64 + 32 * x + 16 * y + 8 * layer,
                    ]
                    .map(|v| v as u8)
                })
            })
        })
        .collect();
    let coordinates = [-1.0f32, 0.0, 0.25, 0.5, 0.75, 1.0, 2.0];
    let mut requests = Vec::new();
    for layer in 0..3 {
        for y in coordinates {
            for x in coordinates {
                requests.extend([x.to_bits(), y.to_bits(), layer, 0]);
            }
        }
    }
    let count = requests.len() as u32 / 4;
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(
        [count, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
    )?;
    let atlas = batch.texture_array_rgba8([2, 2, 3], texels.clone())?;
    let requests_buffer = batch.buffer(requests.iter().flat_map(|v| v.to_le_bytes()).collect())?;
    let mut expected = Vec::new();
    for filter in [
        SamplerFilter::Nearest,
        SamplerFilter::Linear,
        SamplerFilter::Nearest,
    ] {
        let sampler = batch.sampler(filter)?;
        let output = batch.buffer(vec![0x71; (count as usize + 5) * 4])?;
        // SAFETY: all request layers are in range; bounded writes have independent outputs.
        unsafe {
            batch.dispatch(
                "sampler_words",
                &[
                    (0, config),
                    (1, atlas),
                    (2, sampler),
                    (3, requests_buffer),
                    (4, output),
                ],
                [1, count.div_ceil(FINE_WORKGROUP_SIZE), 1],
            )?;
        }
        batch.readback(output)?;
        let mut bytes = Vec::new();
        for q in requests.chunks_exact(4) {
            let x = f32::from_bits(q[0]);
            let y = f32::from_bits(q[1]);
            let sample = |px: i32, py: i32, c: usize| {
                texels[((q[2] * 4 + py.clamp(0, 1) as u32 * 2 + px.clamp(0, 1) as u32) * 4)
                    as usize
                    + c] as f32
            };
            for c in 0..4 {
                let v = match filter {
                    SamplerFilter::Nearest => {
                        sample((x * 2.0).floor() as i32, (y * 2.0).floor() as i32, c)
                    }
                    SamplerFilter::Linear => {
                        let sx = x * 2.0 - 0.5;
                        let sy = y * 2.0 - 0.5;
                        let ix = sx.floor() as i32;
                        let iy = sy.floor() as i32;
                        let fx = sx - sx.floor();
                        let fy = sy - sy.floor();
                        let top = sample(ix, iy, c) * (1.0 - fx) + sample(ix + 1, iy, c) * fx;
                        let bottom =
                            sample(ix, iy + 1, c) * (1.0 - fx) + sample(ix + 1, iy + 1, c) * fx;
                        top * (1.0 - fy) + bottom * fy
                    }
                };
                bytes.push((v + 0.5) as u8);
            }
        }
        bytes.extend([0x71; 20]);
        expected.push(bytes);
    }
    // Switch back to a pipeline with no sampler table or internal grid constants.
    let config = batch.buffer(
        [2u32, 2, 2, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
    )?;
    let output = batch.texture_rgba8([2, 2], vec![0; 16])?;
    // SAFETY: the selected layer and all coordinates are bounded by the array.
    unsafe {
        batch.dispatch(
            "texture_layer",
            &[(0, config), (1, atlas), (2, output)],
            [1, 2, 1],
        )?;
    }
    batch.readback(output)?;
    expected.push(texels[32..48].to_vec());
    routes.check(
        &batch,
        &expected,
        "explicit sampler modes and clamp boundaries",
    )?;
    routes.validate()
}
