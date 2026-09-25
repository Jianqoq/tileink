use super::{Object, Result};
use crate::native::shaders::{BindingKind, NativeShaderArtifact};
use objc2_foundation::NSString;
use objc2_metal::*;
use std::collections::BTreeMap;

mod reflection;
mod render;

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
pub(super) fn slot(slot: u32) -> usize {
    if slot == 31 { 30 } else { slot as usize }
}

#[allow(deprecated)]
pub(super) fn ensure(
    device: &objc2::runtime::ProtocolObject<dyn MTLDevice>,
    libraries: &mut BTreeMap<&'static str, Object<dyn MTLLibrary>>,
    pipelines: &mut BTreeMap<&'static str, Pipeline>,
    shader: &'static NativeShaderArtifact,
) -> Result<()> {
    if pipelines.contains_key(shader.entry) {
        return Ok(());
    }
    if shader.format != "metallib" {
        return Err("Metal requires a compiled MSL library".into());
    }
    if !libraries.contains_key(shader.cache_key) {
        libraries.insert(
            shader.cache_key,
            device.newLibraryWithData_error(&dispatch2::DispatchData::from_bytes(shader.bytes))?,
        );
    }
    let function = libraries[shader.cache_key]
        .newFunctionWithName(&NSString::from_str(shader.entry))
        .ok_or("Metal library entry missing")?;
    let (state, active) = if shader.entry == "fine_tile_main" {
        render::create(device, &libraries[shader.cache_key], &function, shader)?
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
    if active.len() != bindings.len() {
        return Err(format!(
            "Metal {} binding count mismatch: {} vs {}",
            shader.entry,
            active.len(),
            bindings.len()
        )
        .into());
    }
    let mut arguments = BTreeMap::new();
    for binding in bindings {
        let kind = match binding.kind {
            BindingKind::Texture | BindingKind::TextureWrite | BindingKind::TextureArray => {
                MTLArgumentType::Texture
            }
            BindingKind::Sampler => MTLArgumentType::Sampler,
            _ => MTLArgumentType::Buffer,
        };
        let argument = active
            .iter()
            .find(|a| a.r#type() == kind && a.index() == slot(binding.slot))
            .ok_or("Metal resource slot/type mismatch")?;
        if binding.kind == BindingKind::Uniform {
            reflection::uniform(argument, shader, binding.slot)?;
        }
        if binding.kind == BindingKind::TextureTable {
            // SAFETY: this is the reflected argument-buffer slot of the function.
            let encoder = unsafe { function.newArgumentEncoderWithBufferIndex(slot(binding.slot)) };
            arguments.insert(binding.slot, encoder);
        }
    }
    pipelines.insert(shader.entry, Pipeline { state, arguments });
    Ok(())
}
