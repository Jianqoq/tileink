use super::{Object, Result};
use crate::native::shaders::{BindingKind, NativeShaderArtifact};
use objc2_foundation::NSString;
use objc2_metal::*;
use std::collections::BTreeMap;

pub(super) struct Pipeline {
    pub state: Object<dyn MTLComputePipelineState>,
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
    let mut reflection = None;
    // SAFETY: the ABI is verified below before any execution. All callers of
    // ComputeBatch::dispatch additionally validate data-dependent shader indices.
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
    let reflection = reflection.ok_or("Metal reflection unavailable")?;
    let active: Vec<_> = reflection
        .arguments()
        .iter()
        .filter(|a| a.isActive())
        .collect();
    if active.len() != shader.bindings.len() {
        return Err(format!(
            "Metal {} binding count mismatch: {} vs {}",
            shader.entry,
            active.len(),
            shader.bindings.len()
        )
        .into());
    }
    let mut arguments = BTreeMap::new();
    for binding in shader.bindings {
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
        if binding.kind == BindingKind::Uniform
            && argument.bufferDataSize() != binding.size as usize
        {
            return Err(format!(
                "Metal {} uniform {} size mismatch",
                shader.entry, binding.slot
            )
            .into());
        }
        if binding.kind == BindingKind::Uniform {
            let expected = shader
                .uniforms
                .iter()
                .find(|uniform| uniform.slot == binding.slot)
                .ok_or("missing Metal uniform layout")?;
            let mut fields = Vec::new();
            if let Some(structure) = argument.bufferStructType() {
                for member in structure.members() {
                    append_scalars(&mut fields, member.offset() as u32, member.dataType())?;
                }
            } else {
                append_scalars(&mut fields, 0, argument.bufferDataType())?;
            }
            if fields != expected.fields {
                return Err(format!(
                    "Metal {} uniform {} field offsets/types mismatch: {fields:?} vs {:?}",
                    shader.entry, binding.slot, expected.fields
                )
                .into());
            }
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

fn append_scalars(fields: &mut Vec<(u32, u8)>, offset: u32, kind: MTLDataType) -> Result<()> {
    let (scalar, lanes) = match kind {
        MTLDataType::UInt => (0, 1),
        MTLDataType::UInt2 => (0, 2),
        MTLDataType::UInt3 => (0, 3),
        MTLDataType::UInt4 => (0, 4),
        MTLDataType::Int => (1, 1),
        MTLDataType::Int2 => (1, 2),
        MTLDataType::Int3 => (1, 3),
        MTLDataType::Int4 => (1, 4),
        MTLDataType::Float => (2, 1),
        MTLDataType::Float2 => (2, 2),
        MTLDataType::Float3 => (2, 3),
        MTLDataType::Float4 => (2, 4),
        _ => return Err("unsupported Metal uniform scalar type".into()),
    };
    fields.extend((0..lanes).map(|lane| (offset + lane * 4, scalar)));
    Ok(())
}
