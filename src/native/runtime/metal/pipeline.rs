use super::{Object, Result};
use crate::native::shaders::{BindingKind, NativeShaderArtifact, ShaderArtifact};
use objc2_foundation::NSString;
use objc2_metal::*;
use std::collections::BTreeMap;

mod reflection;
mod render;
#[cfg(test)]
mod tests;

pub(super) enum State {
    Compute(Object<dyn MTLComputePipelineState>),
    Tile {
        full: Object<dyn MTLRenderPipelineState>,
        sparse: Object<dyn MTLRenderPipelineState>,
    },
}

pub(super) struct Pipeline {
    pub state: State,
    pub arguments: BTreeMap<u32, Object<dyn MTLArgumentEncoder>>,
}
pub(super) const fn slot(slot: u32) -> usize {
    if slot == 31 { 30 } else { slot as usize }
}

pub(super) fn ensure_pass(
    device: &objc2::runtime::ProtocolObject<dyn MTLDevice>,
    libraries: &mut BTreeMap<String, Object<dyn MTLLibrary>>,
    pipelines: &mut BTreeMap<String, Pipeline>,
    pass: &crate::native::runtime::compute::Pass,
) -> Result<()> {
    ensure_inner(
        device,
        libraries,
        pipelines,
        pass.artifact(),
        pass.key(),
        pass.entry(),
        pass.native_source
            .as_ref()
            .map(|s| s.text(true))
            .transpose()?,
    )
}
pub(super) fn ensure(
    device: &objc2::runtime::ProtocolObject<dyn MTLDevice>,
    libraries: &mut BTreeMap<String, Object<dyn MTLLibrary>>,
    pipelines: &mut BTreeMap<String, Pipeline>,
    shader: &'static NativeShaderArtifact,
) -> Result<()> {
    ensure_inner(
        device,
        libraries,
        pipelines,
        *shader,
        shader.entry,
        shader.entry,
        None,
    )
}

#[allow(
    deprecated,
    reason = "Required Metal reflection/AppKit APIs preserve supported macOS versions."
)]
fn ensure_inner(
    device: &objc2::runtime::ProtocolObject<dyn MTLDevice>,
    libraries: &mut BTreeMap<String, Object<dyn MTLLibrary>>,
    pipelines: &mut BTreeMap<String, Pipeline>,
    shader: ShaderArtifact<'_>,
    key: &str,
    entry: &str,
    source: Option<&str>,
) -> Result<()> {
    if pipelines.contains_key(key) {
        return Ok(());
    }
    if shader.format != "metallib" && source.is_none() {
        return Err("Metal requires a compiled MSL library".into());
    }
    let library_key = if source.is_some() {
        key
    } else {
        shader.cache_key
    };
    let library = if let Some(library) = libraries.get(library_key) {
        library.clone()
    } else if let Some(source) = source {
        let options = MTLCompileOptions::new();
        options.setLanguageVersion(MTLLanguageVersion::Version2_4);
        // Preserve the arithmetic written in the application source.
        options.setFastMathEnabled(false);
        options.setPreserveInvariance(true);
        device.newLibraryWithSource_options_error(&NSString::from_str(source), Some(&options))?
    } else {
        device.newLibraryWithData_error(&dispatch2::DispatchData::from_bytes(shader.bytes))?
    };
    let function = library
        .newFunctionWithName(&NSString::from_str(entry))
        .ok_or("Metal library entry missing")?;
    let (state, active) = if source.is_none() && shader.entry == "fine_tile_main" {
        render::create(device, &library, &function, &shader)?
    } else {
        let mut reflection = None;
        // SAFETY: reflection is checked before any resource is bound.
        let state = unsafe {
            device.newComputePipelineStateWithFunction_options_reflection_error(
                &function,
                MTLPipelineOption::ArgumentInfo | MTLPipelineOption::BufferTypeInfo,
                Some(&mut reflection),
            )?
        };
        if state.maxTotalThreadsPerThreadgroup()
            < shader.workgroup.iter().map(|&n| n as usize).product()
        {
            return Err("Metal kernel exceeds workgroup limit".into());
        }
        let active = reflection
            .ok_or("Metal reflection unavailable")?
            .arguments()
            .iter()
            .filter(|a| a.isActive())
            .collect();
        (State::Compute(state), active)
    };
    let bindings: Vec<_> = shader
        .bindings
        .iter()
        .filter(|b| !matches!(state, State::Tile { .. }) || b.slot != 1)
        .collect();
    if source.is_none() && active.len() != bindings.len() {
        return Err(format!(
            "Metal {} binding count mismatch: {} vs {}",
            shader.entry,
            active.len(),
            bindings.len()
        )
        .into());
    }
    if source.is_some()
        && active.iter().any(|a| {
            !bindings.iter().any(|b| {
                a.index() == slot(b.slot)
                    && a.r#type()
                        == match b.kind {
                            BindingKind::Texture
                            | BindingKind::TextureWrite
                            | BindingKind::TextureReadWrite
                            | BindingKind::TextureArray => MTLArgumentType::Texture,
                            BindingKind::Sampler => MTLArgumentType::Sampler,
                            _ => MTLArgumentType::Buffer,
                        }
            })
        })
    {
        return Err("native user shader declares an unsupported resource binding".into());
    }
    let mut arguments = BTreeMap::new();
    for binding in bindings {
        let kind = match binding.kind {
            BindingKind::Texture
            | BindingKind::TextureWrite
            | BindingKind::TextureReadWrite
            | BindingKind::TextureArray => MTLArgumentType::Texture,
            BindingKind::Sampler => MTLArgumentType::Sampler,
            _ => MTLArgumentType::Buffer,
        };
        let argument = active
            .iter()
            .find(|a| a.r#type() == kind && a.index() == slot(binding.slot));
        let Some(argument) = argument else {
            if source.is_some() && !binding.kind.writable() {
                continue;
            }
            return Err("Metal resource slot/type mismatch".into());
        };
        if source.is_some() {
            let access_valid = match binding.kind {
                BindingKind::TextureWrite => argument.access() == MTLBindingAccess::WriteOnly,
                BindingKind::TextureReadWrite => argument.access() == MTLBindingAccess::ReadWrite,
                BindingKind::Write => matches!(
                    argument.access(),
                    MTLBindingAccess::ReadWrite | MTLBindingAccess::WriteOnly
                ),
                _ => argument.access() == MTLBindingAccess::ReadOnly,
            };
            if !access_valid
                || matches!(
                    binding.kind,
                    BindingKind::Texture
                        | BindingKind::TextureWrite
                        | BindingKind::TextureReadWrite
                ) && (argument.textureType() != MTLTextureType::Type2D
                    || argument.textureDataType() != MTLDataType::Float)
            {
                return Err("native user shader resource access/type mismatch".into());
            }
        }
        if source.is_some()
            && binding.kind == BindingKind::TextureArray
            && (argument.textureType() != MTLTextureType::Type2DArray
                || argument.textureDataType() != MTLDataType::Float)
        {
            return Err("native user shader texture array dimension/type mismatch".into());
        }
        if binding.kind == BindingKind::Uniform {
            reflection::uniform(argument, &shader, binding.slot)?;
        }
        if binding.kind == BindingKind::TextureTable {
            let structure = argument
                .bufferStructType()
                .ok_or("Metal texture table structure missing")?;
            let members = structure.members();
            if members.len() != 1 {
                return Err("Metal texture table must contain one image array".into());
            }
            let array = members
                .objectAtIndex(0)
                .arrayType()
                .ok_or("Metal texture table image array missing")?;
            if array.elementType() != MTLDataType::Texture
                || array.arrayLength() != binding.count as usize
            {
                return Err("Metal texture table descriptor count mismatch".into());
            }
            // SAFETY: this is the reflected argument-buffer slot of the function.
            let encoder = unsafe { function.newArgumentEncoderWithBufferIndex(slot(binding.slot)) };
            arguments.insert(binding.slot, encoder);
        }
    }
    // Cache only validated programs. Missing entries and rejected interfaces must
    // not retain libraries outside the successful-pipeline cache limit.
    libraries.entry(library_key.to_owned()).or_insert(library);
    pipelines.insert(key.to_owned(), Pipeline { state, arguments });
    Ok(())
}
