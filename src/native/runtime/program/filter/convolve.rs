use super::region;
use crate::{
    native::runtime::{
        Result,
        compute::{ComputeBatch, ResourceId},
    },
    shared::filter_config::FilterConfig,
};

#[derive(Clone, Copy, Debug)]
pub struct Kernels {
    buffer: ResourceId,
    len: u32,
}

/// A private buffer/count pair makes every reversed kernel load independently bounded.
pub fn upload(batch: &mut ComputeBatch, weights: &[f32]) -> Result<Kernels> {
    if weights.is_empty() || weights.len() > u32::MAX as usize / 4 {
        return Err("invalid convolution kernel storage length".into());
    }
    // Straight RGB may be as large as 255 for raw nonpremultiplied pixels.
    // Bound the worst possible sum to prevent mixed-sign infinities becoming NaN.
    let magnitude: f64 = weights.iter().map(|v| f64::from(v.abs())).sum();
    if weights.iter().any(|v| !v.is_finite()) || magnitude > f64::from(f32::MAX) / 255.0 {
        return Err("convolution kernel accumulation overflow".into());
    }
    Ok(Kernels {
        buffer: batch.buffer(bytemuck::cast_slice(weights).to_vec())?,
        len: u32::try_from(weights.len())?,
    })
}

pub fn encode(
    batch: &mut ComputeBatch,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    kernels: Kernels,
    source: ResourceId,
    target: ResourceId,
) -> Result<()> {
    if !config.amount.is_finite()
        || !config.rect_x0.is_finite()
        || config.kernel_edge_mode > 2
        || config.kernel_preserve_alpha > 1
    {
        return Err("invalid convolution mode, divisor or bias".into());
    }
    let count = config
        .kernel_columns
        .checked_mul(config.kernel_rows)
        .ok_or("convolution shape overflow")?;
    if config
        .kernel_offset
        .checked_add(count)
        .is_none_or(|end| end > kernels.len)
    {
        return Err("convolution kernel range exceeds storage".into());
    }
    if count != 0
        && (config.kernel_target_x >= config.kernel_columns
            || config.kernel_target_y >= config.kernel_rows)
    {
        return Err("convolution target outside kernel".into());
    }
    // Both the sample coordinate and its relative wrap coordinate must fit i32.
    for (extent, taps) in [
        (config.width, config.kernel_columns),
        (config.height, config.kernel_rows),
    ] {
        if u64::from(extent) + u64::from(taps) > i32::MAX as u64 {
            return Err("convolution signed coordinate overflow".into());
        }
    }
    region::record(
        batch,
        "filter_convolve_matrix_region",
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings {
            textures: &[(1, source)],
            texture_extent: [config.width, config.height],
            buffers: &[(6, kernels.buffer)],
        },
        target,
    )
}
