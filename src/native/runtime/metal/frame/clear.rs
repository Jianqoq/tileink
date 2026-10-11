//! Full-attachment replacement needs no per-pixel compute dispatch. Partial,
//! compact and undersized dispatches must preserve the ordinary shader semantics.
use super::*;
use crate::{
    native::runtime::compute::Pass,
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
    // This is the compiled built-in artifact; application source uses a distinct
    // artifact even when its entry has the same name. ComputeBatch has validated
    // all required slots, the uniform's minimum size and the 2D target dimension.
    let binding = |slot| {
        pass.bindings
            .iter()
            .find(|(b, _)| b.slot == slot)
            .unwrap()
            .1
            .index()
    };
    let (config, target) = (binding(0), binding(3));
    // Inline classification proves the host bytes immutable. A storage alias
    // can be written by an earlier GPU pass and must keep shader-based clearing.
    if !matches!(resources[config], Resource::InlineUniform) {
        return Ok(false);
    }
    // Native uniforms may include trailing storage. Only the reflected header
    // is consumed by the shader, so read that same prefix for attachment clears.
    let bytes = batch.resources()[config].bytes();
    let config =
        bytemuck::pod_read_unaligned::<FilterConfig>(&bytes[..std::mem::size_of::<FilterConfig>()]);
    let target = resources[target].texture()?;
    if !target.usage().contains(MTLTextureUsage::RenderTarget)
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
mod tests;
