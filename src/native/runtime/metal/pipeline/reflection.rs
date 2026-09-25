//! Shared ABI checks for uniforms in compute, vertex, fragment, and tile stages.
use super::*;

#[allow(deprecated)]
pub(super) fn uniform(
    argument: &MTLArgument,
    shader: &NativeShaderArtifact,
    slot: u32,
) -> Result<()> {
    let binding = shader
        .bindings
        .iter()
        .find(|b| b.slot == slot && b.kind == BindingKind::Uniform)
        .ok_or("missing Metal uniform binding")?;
    if argument.bufferDataSize() != binding.size as usize {
        return Err(format!("Metal {} uniform {} size mismatch", shader.entry, slot).into());
    }
    let expected = shader
        .uniforms
        .iter()
        .find(|u| u.slot == slot)
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
            shader.entry, slot, expected.fields
        )
        .into());
    }
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
