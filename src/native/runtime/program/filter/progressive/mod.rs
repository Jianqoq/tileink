//! Progressive UI blur: fixed-work binomial reductions plus one scale-selection pass.
//! All levels keep their own logical extent; pooled padding is never sampled.

use crate::{ProgressiveBlur, ProgressiveBlurQuality};
mod pyramid;
use crate::native::runtime::compute::{
    ComputeBatch, Resource, ResourceId, SamplerFilter, TextureCopy,
};
use crate::shared::bounds::Bounds;
use crate::shared::progressive_blur_config::ProgressiveBlurConfig;
use pyramid::levels;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const TABLE_SIZE: usize = 64;
// SAFETY: repr(C), scalar arrays, no implicit padding, all bit patterns valid.
unsafe impl bytemuck::Zeroable for ProgressiveBlurConfig {}
unsafe impl bytemuck::Pod for ProgressiveBlurConfig {}

fn allocate(batch: &mut ComputeBatch, size: [u32; 2]) -> Result<ResourceId> {
    if let Some(image) = batch.reusable_overwritten_surface(size)? {
        return Ok(image);
    }
    let bytes = (size[0] as usize)
        .checked_mul(size[1] as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("progressive blur allocation overflow")?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(bytes)?;
    pixels.resize(bytes, 0);
    batch.texture_rgba8(size, pixels)
}

fn dispatch(
    batch: &mut ComputeBatch,
    entry: &str,
    config: ProgressiveBlurConfig,
    bindings: &[(u32, ResourceId)],
) -> Result<()> {
    let uniform = batch.buffer(bytemuck::bytes_of(&config).to_vec())?;
    let mut bindings = bindings.to_vec();
    bindings.push((0, uniform));
    let grid = [
        config.output[2].div_ceil(8),
        config.output[3].div_ceil(8),
        1,
    ];
    // SAFETY: encode validates image domains, all pixel owners are unique, reductions
    // guard each source load and resolve guards the bounded level-table indices.
    unsafe { batch.dispatch(entry, &bindings, grid) }
}

pub(crate) fn encode(
    batch: &mut ComputeBatch,
    target: ResourceId,
    size: [u32; 2],
    bounds: Bounds,
    blur: ProgressiveBlur,
) -> Result<()> {
    let gradient = blur
        .projection()
        .ok_or("invalid progressive blur parameters")?;
    batch.size(target)?;
    if !matches!(&batch.resources()[target.index()], Resource::Texture(t)
        if !t.array && t.size[0] >= size[0] && t.size[1] >= size[1])
        || size.contains(&0)
        || size.iter().any(|&n| n > i32::MAX as u32)
    {
        return Err("invalid progressive blur target".into());
    }
    let bounds = bounds.intersect(Bounds::canvas(size[0], size[1]));
    if bounds.is_empty() || blur.max_std_dev == 0.0 {
        return Ok(());
    }
    let extent = [bounds.width(), bounds.height()];
    let levels = levels(extent, blur.max_std_dev, blur.quality);
    if levels.len() > TABLE_SIZE {
        return Err("progressive blur level count overflow".into());
    }
    let sampler = batch.sampler(SamplerFilter::Linear)?;
    let mut images = Vec::with_capacity(TABLE_SIZE);
    let original = allocate(batch, extent)?;
    batch.copy_texture(TextureCopy {
        source: target,
        destination: original,
        source_origin: [bounds.x0 as u32, bounds.y0 as u32, 0],
        destination_origin: [0; 3],
        extent: [extent[0], extent[1], 1],
    })?;
    images.push(original);
    // One scratch image is overwritten between horizontal/vertical pairs.
    // Explicit source extents exclude spare capacity on every later pass.
    let scratch = if levels.len() > 1 {
        Some(allocate(batch, extent)?)
    } else {
        None
    };
    for (i, level) in levels.iter().enumerate().skip(1) {
        let image = allocate(batch, level.size)?;
        let previous = &levels[i - 1];
        let kernel = batch.buffer(bytemuck::cast_slice(&level.kernel).to_vec())?;
        let intermediate = [level.size[0], previous.size[1]];
        for (axis, source, target, source_size, output_size) in [
            (
                0,
                images[i - 1],
                scratch.unwrap(),
                previous.size,
                intermediate,
            ),
            (1, scratch.unwrap(), image, intermediate, level.size),
        ] {
            dispatch(
                batch,
                "progressive_blur_reduce",
                ProgressiveBlurConfig {
                    output: [0, 0, output_size[0], output_size[1]],
                    source: [0, 0, source_size[0], source_size[1]],
                    step: level.step,
                    axis,
                    count: level.kernel.len() as u32,
                    ..Default::default()
                },
                &[(1, source), (2, kernel), (3, target), (13, sampler)],
            )?;
        }
        images.push(image);
    }
    let metadata: Vec<[f32; 4]> = levels
        .iter()
        .map(|l| [l.size[0] as f32, l.size[1] as f32, l.scale, l.variance])
        .collect();
    let metadata = batch.buffer(bytemuck::cast_slice(&metadata).to_vec())?;
    images.resize(TABLE_SIZE, *images.last().unwrap());
    let table = batch.texture_table(&images)?;
    dispatch(
        batch,
        "progressive_blur_resolve",
        ProgressiveBlurConfig {
            output: [bounds.x0 as u32, bounds.y0 as u32, extent[0], extent[1]],
            source: [bounds.x0 as u32, bounds.y0 as u32, extent[0], extent[1]],
            gradient,
            max_std_dev: blur.max_std_dev,
            count: levels.len() as u32,
            ..Default::default()
        },
        &[(2, metadata), (3, target), (13, sampler), (30, table)],
    )
}

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod quality_tests;
#[cfg(test)]
mod tests;
