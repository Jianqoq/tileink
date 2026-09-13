//! Owned, API-neutral resources and ordered compute commands. Native adapters
//! upload each buffer once and keep intermediate values on the GPU until readback.
use super::Result;
use crate::native::shaders::{Binding, BindingKind, NativeShaderArtifact};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT_BATCH: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BufferId {
    owner: u64,
    index: usize,
}
impl BufferId {
    pub fn index(self) -> usize {
        self.index
    }
}
pub struct Buffer {
    pub bytes: Vec<u8>,
}
pub struct Pass {
    pub shader: &'static NativeShaderArtifact,
    pub bindings: Vec<(Binding, BufferId)>,
    pub grid: [u32; 3],
}
pub struct ComputeBatch {
    owner: u64,
    buffers: Vec<Buffer>,
    passes: Vec<Pass>,
    outputs: Vec<BufferId>,
}
impl ComputeBatch {
    pub fn new() -> Self {
        let owner = NEXT_BATCH
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("native batch identity exhausted");
        Self {
            owner,
            buffers: Vec::new(),
            passes: Vec::new(),
            outputs: Vec::new(),
        }
    }
    pub fn buffer(&mut self, bytes: Vec<u8>) -> Result<BufferId> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(4) || bytes.len() > u32::MAX as usize {
            return Err("invalid native compute buffer size/alignment".into());
        }
        let id = BufferId {
            owner: self.owner,
            index: self.buffers.len(),
        };
        self.buffers.push(Buffer { bytes });
        Ok(id)
    }
    pub fn size(&self, id: BufferId) -> Result<usize> {
        if id.owner != self.owner {
            return Err("buffer belongs to another compute batch".into());
        }
        self.buffers
            .get(id.index)
            .map(|b| b.bytes.len())
            .ok_or_else(|| "invalid compute buffer".into())
    }
    /// # Safety
    /// The stage encoder must validate data-dependent indices, write ownership
    /// and barrier-uniform control flow for this kernel. This method additionally
    /// rejects invalid handles, binding layouts, physical sizes and launch grids.
    pub unsafe fn dispatch(
        &mut self,
        entry: &str,
        bindings: &[(u32, BufferId)],
        grid: [u32; 3],
    ) -> Result<()> {
        let shader = crate::NATIVE_SHADER_ARTIFACTS
            .iter()
            .find(|a| a.entry == entry && !a.bindings.is_empty())
            .ok_or("unknown native compute program")?;
        if grid.iter().any(|n| *n > 65535)
            || grid.iter().map(|v| *v as u64).product::<u64>() > u32::MAX as u64
        {
            return Err("native compute dispatch exceeds dimensions/addressing".into());
        }
        let expected: Vec<_> = shader.bindings.iter().filter(|b| !b.internal).collect();
        if bindings.len() != expected.len() {
            return Err("native compute binding count mismatch".into());
        }
        let mut ordered = Vec::new();
        for &binding in expected {
            let mut matches = bindings.iter().filter(|(slot, _)| *slot == binding.slot);
            let id = matches.next().ok_or("missing native compute binding")?.1;
            if matches.next().is_some() || self.size(id)? < (binding.size as usize) {
                return Err("duplicate or undersized native compute binding".into());
            }
            if ordered
                .iter()
                .any(|(other, other_id): &(Binding, BufferId)| {
                    *other_id == id
                        && (binding.kind == BindingKind::Write || other.kind == BindingKind::Write)
                })
            {
                return Err("aliased native writable bindings".into());
            }
            ordered.push((binding, id));
        }
        if grid.contains(&0) {
            return Ok(());
        }
        self.passes.push(Pass {
            shader,
            bindings: ordered,
            grid,
        });
        Ok(())
    }
    pub fn readback(&mut self, id: BufferId) -> Result<usize> {
        self.size(id)?;
        if let Some(index) = self.outputs.iter().position(|old| *old == id) {
            return Ok(index);
        }
        let index = self.outputs.len();
        self.outputs.push(id);
        Ok(index)
    }
    pub fn buffers(&self) -> &[Buffer] {
        &self.buffers
    }
    pub fn passes(&self) -> &[Pass] {
        &self.passes
    }
    pub fn outputs(&self) -> &[BufferId] {
        &self.outputs
    }
}
#[cfg(test)]
#[path = "tests/compute.rs"]
mod tests;
