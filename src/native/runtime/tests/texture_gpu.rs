use super::four_api::Routes;
use crate::native::runtime::{Result, compute::ComputeBatch};
use crate::shared::gpu_constants::FINE_WORKGROUP_SIZE;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_textures_preserve_multiline_uploads_and_read_after_write() -> Result<()> {
    let routes = Routes::new()?;
    for [width, height] in [[1u32, 3u32], [3, 7], [65, 9]] {
        let mut source = Vec::new();
        for y in 0..height {
            for x in 0..width {
                source.extend([
                    (x + y * width) as u8,
                    (x * 31) as u8,
                    (y * 67) as u8,
                    (x * 17 + y * 13) as u8,
                ]);
            }
        }
        let extent = [width + 1, height + 1];
        let untouched = vec![0x37; (extent[0] * extent[1] * 4) as usize];
        let mut first = untouched.clone();
        let mut second = untouched.clone();
        for y in 0..height {
            for x in 0..width {
                let dst = ((y * extent[0] + x) * 4) as usize;
                let src = (((height - 1 - y) * width + width - 1 - x) * 4) as usize;
                first[dst..dst + 4].copy_from_slice(&[
                    source[src + 2],
                    source[src + 1],
                    source[src],
                    source[src + 3],
                ]);
                let src = ((y * width + x) * 4) as usize;
                second[dst..dst + 4].copy_from_slice(&source[src..src + 4]);
            }
        }
        let mut batch = ComputeBatch::new();
        let guard_bytes = vec![0x12, 0x34, 0x56, 0x78];
        let guard = batch.buffer(guard_bytes.clone())?;
        let config = batch.buffer(
            [width, height, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect(),
        )?;
        let input = batch.texture_rgba8([width, height], source)?;
        let output = batch.texture_rgba8(extent, untouched.clone())?;
        let restored = batch.texture_rgba8(extent, untouched)?;
        // SAFETY: explicit bounds cover the source and both larger destinations;
        // each invocation writes one pixel, with no source/destination alias.
        unsafe {
            batch.dispatch(
                "texture_flip",
                &[(0, config), (1, input), (2, output)],
                [width.div_ceil(FINE_WORKGROUP_SIZE), height, 1],
            )?;
            batch.dispatch(
                "texture_flip",
                &[(0, config), (1, output), (2, restored)],
                [width.div_ceil(FINE_WORKGROUP_SIZE), height, 1],
            )?;
        }
        batch.readback(guard)?;
        batch.readback(output)?;
        batch.readback(config)?;
        batch.readback(restored)?;
        routes.check(
            &batch,
            &[
                guard_bytes,
                first,
                [width, height, 0, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
                second,
            ],
            "RGBA8 texture rows and read-after-write",
        )?;
    }
    // No uniform buffers or compute passes: only upload, transition and readback.
    let mut batch = ComputeBatch::new();
    let bytes: Vec<u8> = (0..24).collect();
    let texture = batch.texture_rgba8([3, 2], bytes.clone())?;
    batch.readback(texture)?;
    routes.check(&batch, &[bytes], "upload-only texture without buffer arena")?;
    routes.validate()
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_texture_arrays_preserve_layers_and_single_layer_views() -> Result<()> {
    let routes = Routes::new()?;
    for [width, height, layers] in [[1u32, 3u32, 3u32], [3, 7, 1], [65, 9, 4]] {
        let source: Vec<u8> = (0..width * height * layers * 4)
            .map(|i| (i.wrapping_mul(37) ^ (i / (width * height * 4)).wrapping_mul(83)) as u8)
            .collect();
        let mut batch = ComputeBatch::new();
        let input = batch.texture_array_rgba8([width, height, layers], source.clone())?;
        batch.readback(input)?;
        let mut expected = vec![source.clone()];
        for layer in (0..layers).rev() {
            let config = batch.buffer(
                [width, height, layer, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
            )?;
            let output =
                batch.texture_rgba8([width, height], vec![0; (width * height * 4) as usize])?;
            // SAFETY: selected layer is in range; each bounded invocation owns one destination pixel.
            unsafe {
                batch.dispatch(
                    "texture_layer",
                    &[(0, config), (1, input), (2, output)],
                    [width.div_ceil(FINE_WORKGROUP_SIZE), height, 1],
                )?;
            }
            batch.readback(output)?;
            let start = (layer * width * height * 4) as usize;
            expected.push(source[start..start + (width * height * 4) as usize].to_vec());
        }
        routes.check(
            &batch,
            &expected,
            "array layers and explicit single-layer array view",
        )?;
    }
    routes.validate()
}
