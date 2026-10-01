//! Full-attachment replacement needs no per-pixel compute dispatch. Partial,
//! compact and undersized dispatches must preserve the ordinary shader semantics.
use super::*;
use crate::{
    native::runtime::compute::{Pass, Resource as Input},
    shared::{filter_config::FilterConfig, gpu_constants::FILTER_WORKGROUP_SIZE},
};

pub(super) fn encode(
    command: &objc2::runtime::ProtocolObject<dyn MTLCommandBuffer>,
    batch: &ComputeBatch,
    pass: &Pass,
    resources: &[Resource],
) -> Result<bool> {
    if pass.shader.entry != "filter_clear_region" {
        return Ok(false);
    }
    let binding = |slot| {
        pass.bindings
            .iter()
            .find(|(b, _)| b.slot == slot)
            .map(|(_, id)| id.index())
    };
    let (Some(config), Some(target)) = (binding(0), binding(3)) else {
        return Ok(false);
    };
    // Inline classification proves the host bytes immutable. A storage alias
    // can be written by an earlier GPU pass and must keep shader-based clearing.
    if !matches!(resources[config], Resource::InlineUniform) {
        return Ok(false);
    }
    let Input::Buffer(bytes) = &batch.resources()[config] else {
        return Ok(false);
    };
    let Ok(config) = bytemuck::try_pod_read_unaligned::<FilterConfig>(bytes) else {
        return Ok(false);
    };
    let target = resources[target].texture()?;
    if !target.usage().contains(MTLTextureUsage::RenderTarget)
        || target.textureType() != MTLTextureType::Type2D
        || pass.shader.workgroup != [FILTER_WORKGROUP_SIZE, 1, 1]
        || !full_attachment(&config, pass.grid, [target.width(), target.height()])
    {
        return Ok(false);
    }
    let descriptor = MTLRenderPassDescriptor::new();
    // SAFETY: attachment zero is valid and this pass replaces the complete texture.
    let color = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    color.setTexture(Some(target));
    color.setLoadAction(MTLLoadAction::Clear);
    color.setStoreAction(MTLStoreAction::Store);
    let [r, g, b, a] = config
        .clear_color
        .to_le_bytes()
        .map(|byte| f64::from(byte) / 255.0);
    color.setClearColor(MTLClearColor {
        red: r,
        green: g,
        blue: b,
        alpha: a,
    });
    command
        .renderCommandEncoderWithDescriptor(&descriptor)
        .ok_or("Metal clear encoder failed")?
        .endEncoding();
    Ok(true)
}

fn full_attachment(c: &FilterConfig, grid: [u32; 3], size: [usize; 2]) -> bool {
    let pixels = u64::from(c.width) * u64::from(c.height);
    if pixels == 0
        || pixels != u64::from(c.pixel_count)
        || size != [c.width as usize, c.height as usize]
        || c.compact_tiles != 0
        || c.region_x0 != 0
        || c.region_y0 != 0
        || c.region_width != c.width
        || c.region_height != c.height
        || c.dispatch_width == 0
        || grid[0] != c.dispatch_width
        || grid[2] != 1
    {
        return false;
    }
    let groups = c.pixel_count.div_ceil(FILTER_WORKGROUP_SIZE);
    grid[0] <= groups && grid[1] == groups.div_ceil(grid[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
    fn attachment_clear_preserves_all_channel_bytes_and_partial_dispatch_pixels() -> Result<()> {
        use crate::native::runtime::program::filter::{self, BasicFilter};
        let mut metal = Metal::with_options(&crate::NativeContextOptions {
            validation: true,
            ..Default::default()
        })?;
        let mut batch = ComputeBatch::new();
        let mut expected = Vec::new();
        // Cover every representable UNORM byte in each channel, including alpha
        // and colors whose RGB exceeds alpha; clear does not premultiply colors.
        for value in 0u8..=255 {
            let color = [value, value.rotate_left(2), value.wrapping_mul(3), !value];
            let target = batch.texture_rgba8([17, 19], [61, 73, 87, 99].repeat(323))?;
            let mut c = config();
            c.clear_color = u32::from_le_bytes(color);
            filter::encode(&mut batch, BasicFilter::Clear, c, None, None, target)?;
            batch.readback(target)?;
            expected.push(color.repeat(323));
        }
        let old = [61, 73, 87, 99];
        let color = [3, 7, 11, 173];
        // Oversized physical texture and undersized raw dispatch must fall back.
        for (size, grid) in [([32, 32], [2, 1, 1]), ([17, 19], [1, 1, 1])] {
            let count = (size[0] * size[1]) as usize;
            let target = batch.texture_rgba8(size, old.repeat(count))?;
            let mut c = config();
            c.clear_color = u32::from_le_bytes(color);
            let uniform = batch.buffer(bytemuck::bytes_of(&c).to_vec())?;
            let tiles = batch.buffer(vec![0; 4])?;
            // SAFETY: each invoked lane has one in-bounds pixel; the short grid
            // intentionally covers only the first 256 of 323 logical pixels.
            unsafe {
                batch.dispatch(
                    "filter_clear_region",
                    &[(0, uniform), (3, target), (8, tiles)],
                    grid,
                )?;
            }
            batch.readback(target)?;
            let mut pixels = old.repeat(count);
            let covered = if grid[0] == 1 { 256 } else { 323 };
            for index in 0..covered {
                let at = ((index / 17) * size[0] as usize + index % 17) * 4;
                pixels[at..at + 4].copy_from_slice(&color);
            }
            expected.push(pixels);
        }
        let target = batch.texture_rgba8([17, 19], old.repeat(323))?;
        let mut c = config();
        c.clear_color = u32::from_le_bytes(color);
        filter::encode(&mut batch, BasicFilter::Clear, c, Some(&[0]), None, target)?;
        batch.readback(target)?;
        let mut pixels = old.repeat(323);
        for y in 0..16 {
            for x in 0..16 {
                pixels[(y * 17 + x) * 4..(y * 17 + x) * 4 + 4].copy_from_slice(&color);
            }
        }
        expected.push(pixels);
        for pass in batch.passes() {
            pipeline::ensure(
                &metal.device,
                &mut metal.libraries,
                &mut metal.pipelines,
                pass.shader,
            )?;
        }
        let frame = Frame::record(&metal, &batch)?;
        drop(batch);
        frame.command.commit();
        frame.wait()?;
        assert_eq!(frame.readback()?, expected);
        metal.assert_valid()
    }

    #[test]
    #[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
    fn attachment_clear_observes_gpu_written_uniform() -> Result<()> {
        let mut metal = Metal::with_options(&crate::NativeContextOptions {
            validation: true,
            ..Default::default()
        })?;
        let mut batch = ComputeBatch::new();
        let mut c = config();
        c.clear_color = u32::from_le_bytes([1, 2, 3, 4]);
        let color = [5, 7, 11, 17];
        let offset = u32::from_le_bytes(color).wrapping_sub(c.clear_color);
        let uniform = batch.buffer(bytemuck::bytes_of(&c).to_vec())?;
        let scan = batch.buffer(bytemuck::cast_slice(&[1u32, 1, 0, 0]).to_vec())?;
        let color_word = std::mem::offset_of!(FilterConfig, clear_color) as u32 / 4;
        let starts = batch.buffer(color_word.to_le_bytes().to_vec())?;
        let lengths = batch.buffer(1u32.to_le_bytes().to_vec())?;
        let offsets = batch.buffer(offset.to_le_bytes().to_vec())?;
        let tiles = batch.buffer(vec![0; 4])?;
        let target = batch.texture_rgba8([17, 19], vec![0; 323 * 4])?;
        // SAFETY: one scan lane changes only clear_color in this allocated
        // config. The subsequent full clear must see that GPU write, not the
        // original host bytes; storage aliases cannot use attachment clearing.
        unsafe {
            batch.dispatch(
                "cumsum_apply_chunk_offsets",
                &[
                    (0, scan),
                    (1, starts),
                    (2, lengths),
                    (5, uniform),
                    (7, offsets),
                ],
                [1, 1, 1],
            )?;
            batch.dispatch(
                "filter_clear_region",
                &[(0, uniform), (3, target), (8, tiles)],
                [2, 1, 1],
            )?;
        }
        batch.readback(target)?;
        for pass in batch.passes() {
            pipeline::ensure(
                &metal.device,
                &mut metal.libraries,
                &mut metal.pipelines,
                pass.shader,
            )?;
        }
        let frame = Frame::record(&metal, &batch)?;
        drop(batch);
        frame.command.commit();
        frame.wait()?;
        assert_eq!(frame.readback()?, [color.repeat(323)]);
        metal.assert_valid()
    }

    fn config() -> FilterConfig {
        FilterConfig {
            width: 17,
            height: 19,
            region_width: 17,
            region_height: 19,
            pixel_count: 323,
            dispatch_width: 2,
            ..Default::default()
        }
    }

    #[test]
    fn attachment_clear_requires_full_dense_dispatch_and_exact_texture_extent() {
        let c = config();
        assert!(full_attachment(&c, [2, 1, 1], [17, 19]));
        let mut narrow = c;
        narrow.dispatch_width = 1;
        assert!(full_attachment(&narrow, [1, 2, 1], [17, 19]));
        for grid in [[1, 1, 1], [2, 0, 1], [2, 1, 2], [2, 2, 1]] {
            assert!(!full_attachment(&c, grid, [17, 19]));
        }
        for size in [[32, 19], [17, 32], [16, 19]] {
            assert!(!full_attachment(&c, [2, 1, 1], size));
        }
        for field in 0..8 {
            let mut changed = c;
            match field {
                0 => changed.compact_tiles = 1,
                1 => changed.region_x0 = 1,
                2 => changed.region_y0 = 1,
                3 => changed.region_width -= 1,
                4 => changed.region_height -= 1,
                5 => changed.pixel_count -= 1,
                6 => changed.dispatch_width = 0,
                _ => changed.width = 0,
            }
            assert!(!full_attachment(&changed, [2, 1, 1], [17, 19]));
        }
    }
}
