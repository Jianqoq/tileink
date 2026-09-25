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
        let mut eligible = vec![true; batch.resources().len()];
        let mut used = vec![false; eligible.len()];
        for pass in batch.passes() {
            for (binding, id) in &pass.bindings {
                if binding.kind == BindingKind::Uniform {
                    used[id.index()] = true;
                } else {
                    eligible[id.index()] = false;
                }
            }
        }
        for output in batch.outputs() {
            eligible[output.index()] = false;
        }
        let mut bytes = Vec::new();
        let mut offsets = vec![None; eligible.len()];
        for (index, resource) in batch.resources().iter().enumerate() {
            if !eligible[index] || !used[index] || matches!(resource, Resource::PersistentBuffer(_))
            {
                continue;
            }
            let Resource::Buffer(data) = resource else {
                return Err("native uniform must be a buffer".into());
            };
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

#[cfg(test)]
#[path = "../tests/uniforms.rs"]
mod tests;
