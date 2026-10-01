use super::{ComputeBatch, Resource};
use crate::native::{runtime::Result, shaders::BindingKind};

/// Immutable constant data can share an upload allocation. Storage aliases and
/// explicit readbacks keep their original allocation and synchronization semantics.
pub(crate) struct Uniforms {
    pub bytes: Vec<u8>,
    pub offsets: Vec<Option<u64>>,
}

impl Uniforms {
    pub fn new(batch: &ComputeBatch, alignment: usize) -> Result<Self> {
        if !alignment.is_power_of_two() {
            return Err("invalid native uniform alignment".into());
        }
        let mut bytes = Vec::new();
        let mut offsets = vec![None; batch.resources().len()];
        for index in immutable_uniforms(batch) {
            let data = batch.resources()[index].bytes();
            let padded = data
                .len()
                .checked_add(alignment - 1)
                .ok_or("native uniform size overflow")?
                & !(alignment - 1);
            let end = bytes
                .len()
                .checked_add(padded)
                .ok_or("native uniform arena overflow")?;
            bytes.try_reserve(padded)?;
            let start = bytes.len();
            bytes.resize(end, 0);
            bytes[start..start + data.len()].copy_from_slice(data);
            offsets[index] = Some(u64::try_from(start)?);
        }
        Ok(Self { bytes, offsets })
    }
}

/// All uses must be constant reads, with no storage alias or CPU readback.
/// Shared by packed DX12/Vulkan uploads and Metal's copied encoder constants.
pub(crate) fn immutable_uniforms(batch: &ComputeBatch) -> impl Iterator<Item = usize> + '_ {
    let mut uses = vec![0u8; batch.resources().len()];
    for pass in batch.passes() {
        for (binding, id) in &pass.bindings {
            uses[id.index()] |= if binding.kind == BindingKind::Uniform {
                1
            } else {
                2
            };
        }
    }
    for id in batch.outputs() {
        uses[id.index()] |= 2;
    }
    uses.into_iter()
        .enumerate()
        .filter_map(move |(index, uses)| {
            (uses == 1 && matches!(batch.resources()[index], Resource::Buffer(_))).then_some(index)
        })
}

#[cfg(test)]
#[path = "../tests/uniforms.rs"]
mod tests;
