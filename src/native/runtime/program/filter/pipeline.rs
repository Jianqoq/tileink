//! Built-in stage frontend. All descriptors below use the public shader API.
//! Native scheduling and binding validation belong to the shared executor.
use super::{ComputeBatch, ResourceId, Result};
use crate::{
    ShaderBindingKind as B, ShaderDispatch, ShaderKernel, ShaderPass, ShaderPipeline,
    ShaderResource, ShaderResourceKind as K,
};
use std::cell::RefCell;
thread_local! { static PLANS: RefCell<rustc_hash::FxHashMap<&'static str, ShaderPipeline>> = RefCell::default(); }
pub(crate) fn record(
    batch: &mut ComputeBatch,
    entry: &str,
    bindings: &[(u32, ResourceId)],
    grid: [u32; 3],
) -> Result<()> {
    let pipeline = PLANS.with(|plans| -> Result<ShaderPipeline> {
        let mut plans = plans.borrow_mut();
        if let Some(p) = plans.get(entry)
            && p.passes()[0].dispatch == ShaderDispatch::Groups(grid)
        {
            return Ok(p.clone());
        }
        let artifact = crate::NATIVE_SHADER_ARTIFACTS
            .iter()
            .find(|a| a.entry == entry && !a.bindings.is_empty())
            .ok_or_else(|| format!("unknown filter pipeline kernel: {entry}"))?;
        let kernel = ShaderKernel::precompiled(artifact)?;
        let resources = kernel
            .bindings()
            .iter()
            .enumerate()
            .map(|(index, b)| ShaderResource::External {
                index,
                kind: match b.kind {
                    B::Config | B::Uniform(_) | B::ReadBuffer | B::ReadWriteBuffer => K::Buffer,
                    B::Texture => K::Texture(crate::ShaderTextureFormat::Rgba8Unorm),
                    B::WriteTexture(f) | B::ReadWriteTexture(f) => K::Texture(f),
                    B::TextureArray => K::TextureArray,
                    B::TextureTable(n) => K::TextureTable(n),
                    B::Sampler => K::Sampler,
                },
            })
            .collect();
        let pass_bindings = kernel
            .bindings()
            .iter()
            .enumerate()
            .map(|(i, b)| (b.slot, i))
            .collect();
        let pipeline = ShaderPipeline::new(
            resources,
            vec![ShaderPass {
                kernel,
                bindings: pass_bindings,
                dispatch: ShaderDispatch::Groups(grid),
            }],
        )?;
        plans.insert(artifact.entry, pipeline.clone());
        Ok(pipeline)
    })?;
    if bindings.len() != pipeline.resources().len() {
        return Err("pipeline invocation binding count mismatch".into());
    }
    let resources = pipeline.passes()[0]
        .bindings
        .iter()
        .map(|&(slot, _)| {
            let mut ids = bindings.iter().filter(|(s, _)| *s == slot);
            let id = ids.next().ok_or("pipeline invocation binding missing")?.1;
            if ids.next().is_some() {
                return Err("duplicate pipeline invocation binding".into());
            }
            Ok(id)
        })
        .collect::<Result<Vec<_>>>()?;
    crate::native::runtime::program_shader::pipeline::record_bindings(batch, &pipeline, &resources)
}
