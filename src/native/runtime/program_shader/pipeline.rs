//! Native workload scheduling only. Source code is compiled verbatim; scheduling
//! never parses effect math or inserts shader instructions.
use super::{NativeSource, Result, layout};
use crate::native::runtime::compute::{ComputeBatch, ResourceId, SamplerFilter};
use crate::shared::shader::ShaderKernelCode;
use crate::{
    ShaderConfig, ShaderDispatch, ShaderKernel, ShaderPipeline, ShaderResource as R, ShaderSampler,
};
#[cfg(test)]
mod builtin_tests;

mod source;
pub(super) use source::source;

pub(crate) fn record(
    batch: &mut ComputeBatch,
    pipeline: &ShaderPipeline,
    region: crate::shared::filter_config::FilterConfig,
    tiles: Option<&[u32]>,
    inputs: &[ResourceId],
    output: ResourceId,
) -> Result<()> {
    if pipeline.input_count() != inputs.len() {
        return Err("shader pipeline input mismatch".into());
    }
    record_output(
        batch,
        pipeline,
        region,
        tiles,
        inputs,
        super::Output {
            image: output,
            origin: [0, 0],
            extent: [region.width, region.height],
        },
    )
}

pub(crate) fn record_output(
    batch: &mut ComputeBatch,
    pipeline: &ShaderPipeline,
    region: crate::shared::filter_config::FilterConfig,
    tiles: Option<&[u32]>,
    inputs: &[ResourceId],
    output: super::Output,
) -> Result<()> {
    if !pipeline.is_canvas_filter() || pipeline.input_count() != inputs.len() {
        return Err("shader pipeline canvas invocation mismatch".into());
    }
    config::validate_output(batch, region, inputs, &output)?;
    record_invocation(
        batch,
        pipeline,
        Some(region),
        tiles,
        inputs,
        Some(output),
        &[],
    )
}

/// Shared native invocation entry, also exposed by NativeShaderBatch. Public
/// external descriptors never capture frame handles inside cached pass plans.
pub(crate) fn record_bindings(
    batch: &mut ComputeBatch,
    pipeline: &ShaderPipeline,
    resources: &[ResourceId],
) -> Result<()> {
    if pipeline.external_count() != resources.len() {
        return Err("pipeline external resource count mismatch".into());
    }
    for (i, r) in pipeline.resources.iter().enumerate() {
        if let R::External { index, kind } = r {
            let id = resources[*index];
            batch.size(id)?;
            use crate::ShaderResourceKind as K;
            use crate::native::runtime::compute::Resource;
            let valid = match (&batch.resources()[id.index()], kind) {
                (Resource::Buffer(_) | Resource::PersistentBuffer(_), K::Buffer)
                | (Resource::Sampler(_), K::Sampler) => true,
                (Resource::Texture(t), K::Texture(f)) => !t.array && t.format == *f,
                (Resource::Texture(t), K::TextureArray) => t.array,
                (Resource::TextureTable(t), K::TextureTable(n)) => t.len() == *n as usize,
                _ => false,
            };
            if !valid {
                return Err("pipeline external resource kind mismatch".into());
            }
        }
        if !matches!(
            r,
            R::External { .. } | R::Uniform(_) | R::Buffer(_) | R::Sampler(_)
        ) {
            return Err(format!("native invocation resource {i} requires a canvas surface").into());
        }
    }
    record_invocation(batch, pipeline, None, None, &[], None, resources)
}

mod config;
fn record_invocation(
    batch: &mut ComputeBatch,
    pipeline: &ShaderPipeline,
    region: Option<crate::shared::filter_config::FilterConfig>,
    tiles: Option<&[u32]>,
    inputs: &[ResourceId],
    output: Option<super::Output>,
    bound: &[ResourceId],
) -> Result<()> {
    // Last-use reuse reduces memory without overlapping simultaneously live
    // intermediates. Aliases share ResourceId so all backend hazard tracking sees them.
    let last: Vec<_> = (0..pipeline.resources.len())
        .map(|i| {
            pipeline
                .passes
                .iter()
                .rposition(|p| p.bindings.iter().any(|&(_, r)| r == i))
        })
        .collect();
    let mut allocated = vec![None; pipeline.resources.len()];
    let mut available = Vec::new();
    for (pass_index, pass) in pipeline.passes.iter().enumerate() {
        let grid = match pass.dispatch {
            ShaderDispatch::Pixels => {
                config::pixel_grid(region.ok_or("pixel dispatch requires a surface")?, tiles)?
            }
            ShaderDispatch::Groups(groups) => groups,
            ShaderDispatch::Image(e) => {
                let region = region.ok_or("pipeline image dispatch requires a surface")?;
                let surface = [region.width, region.height];
                let size = e.resolve(surface);
                [
                    size[0].div_ceil(pass.kernel.workgroup[0]),
                    size[1].div_ceil(pass.kernel.workgroup[1]),
                    1,
                ]
            }
            ShaderDispatch::Region { extent, outset } => {
                let region = region.ok_or("pipeline region dispatch requires a surface")?;
                let [x0, y0, x1, y1] = region_bounds(region, extent, outset);
                [
                    (x1 - x0).div_ceil(pass.kernel.workgroup[0]),
                    (y1 - y0).div_ceil(pass.kernel.workgroup[1]),
                    1,
                ]
            }
        };
        let mut bindings = Vec::new();
        for (i, r) in pipeline.resources.iter().enumerate() {
            if matches!(r, R::Config(_) | R::ActiveTiles) {
                allocated[i] = None;
            }
        }
        for &(slot, index) in &pass.bindings {
            let id = if let Some(id) = allocated[index] {
                id
            } else {
                let id = match &pipeline.resources[index] {
                    R::External { index, .. } => bound[*index],
                    R::Uniform(bytes) => batch.buffer(bytes.to_vec())?,
                    R::Buffer(words) => batch.shader_buffer(words)?,
                    R::Sampler(f) => batch.sampler(match f {
                        ShaderSampler::Nearest => SamplerFilter::Nearest,
                        ShaderSampler::Linear => SamplerFilter::Linear,
                    })?,
                    resource => {
                        let region = region.ok_or("pipeline resource requires a surface")?;
                        let surface = [region.width, region.height];
                        match resource {
                            R::Input(i) => inputs[*i],
                            R::Output => output.as_ref().ok_or("pipeline output missing")?.image,
                            R::ActiveTiles => {
                                config::active_tiles(batch, region, tiles, pass.dispatch)?
                            }
                            R::Image {
                                size,
                                format,
                                bytes,
                            } => batch.shader_image(*size, *format, bytes)?,
                            R::Texture { extent, format } => {
                                let size = extent.resolve(surface);
                                if let Some(i) = available
                                    .iter()
                                    .position(|&(s, f, _)| s == size && f == *format)
                                {
                                    available.swap_remove(i).2
                                } else {
                                    batch.shader_texture(size, *format)?
                                }
                            }
                            R::Config(c) => config::buffer(
                                batch,
                                pipeline,
                                *c,
                                region,
                                tiles,
                                pass.dispatch,
                                grid,
                                output.as_ref(),
                            )?,
                            _ => unreachable!(),
                        }
                    }
                };
                allocated[index] = Some(id);
                id
            };
            bindings.push((slot, id));
        }
        match &pass.kernel.code {
            ShaderKernelCode::Source { .. } => batch.dispatch_with_shader(
                &layout::SOURCE,
                Some(source(&pass.kernel)),
                &bindings,
                grid,
            )?,
            ShaderKernelCode::Compiled(artifact) => {
                batch.dispatch_with_shader(artifact, None, &bindings, grid)?
            }
        }
        for (index, r) in pipeline.resources.iter().enumerate() {
            if last[index] == Some(pass_index)
                && let R::Texture { extent, format } = r
            {
                let region = region.ok_or("pipeline texture requires a surface")?;
                let surface = [region.width, region.height];
                available.push((extent.resolve(surface), *format, allocated[index].unwrap()));
            }
        }
    }
    Ok(())
}

/// Round outward in the stable full-surface decimation grid. Cropping allocations
/// instead would change resampling phase on retained updates and moving regions.
fn region_bounds(
    region: crate::shared::filter_config::FilterConfig,
    extent: crate::ShaderExtent,
    outset: u32,
) -> [u32; 4] {
    let surface = [region.width, region.height];
    let start = [region.region_x0, region.region_y0];
    let length = [region.region_width, region.region_height];
    let mut result = [0; 4];
    for axis in 0..2 {
        let a = start[axis].saturating_sub(outset).min(surface[axis]);
        let b = start[axis]
            .saturating_add(length[axis])
            .saturating_add(outset)
            .min(surface[axis]);
        // Surface-relative divisors use ceil(surface/divisor), so proportional
        // mapping would shift odd-size phases. Resolve the actual dispatch grid.
        (result[axis], result[axis + 2]) = match extent {
            crate::ShaderExtent::Surface { divisor } => {
                (a / divisor[axis], b.div_ceil(divisor[axis]))
            }
            crate::ShaderExtent::Fixed(size) => (
                (u64::from(a) * u64::from(size[axis]) / u64::from(surface[axis])) as u32,
                (u64::from(b) * u64::from(size[axis])).div_ceil(u64::from(surface[axis])) as u32,
            ),
        };
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pipeline_region_rounds_in_the_original_divisor_grid_and_clamps_halos() {
        let region = crate::shared::filter_config::FilterConfig {
            width: 17,
            height: 9,
            region_x0: 15,
            region_y0: 7,
            region_width: 1,
            region_height: 1,
            ..Default::default()
        };
        assert_eq!(
            region_bounds(region, crate::ShaderExtent::downsample(2), 0),
            [7, 3, 8, 4]
        );
        assert_eq!(
            region_bounds(region, crate::ShaderExtent::downsample(2), u32::MAX),
            [0, 0, 9, 5]
        );
        assert_eq!(
            region_bounds(region, crate::ShaderExtent::Fixed([3, 5]), 0),
            [2, 3, 3, 5]
        );
    }
}
