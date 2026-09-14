//! Owned, API-neutral resources and ordered compute commands. Native adapters
//! upload each buffer once and keep intermediate values on the GPU until readback.
use super::Result;
use crate::native::shaders::{Binding, BindingKind, NativeShaderArtifact};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT_BATCH: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ResourceId {
    owner: u64,
    index: usize,
}
impl ResourceId {
    pub fn index(self) -> usize {
        self.index
    }
}
#[path = "compute/resource.rs"]
mod resource;
pub use resource::{Resource, SamplerFilter, Texture};
pub struct Pass {
    pub shader: &'static NativeShaderArtifact,
    pub bindings: Vec<(Binding, ResourceId)>,
    pub grid: [u32; 3],
}
pub struct ComputeBatch {
    owner: u64,
    resources: Vec<Resource>,
    passes: Vec<Pass>,
    outputs: Vec<ResourceId>,
}
impl ComputeBatch {
    pub fn new() -> Self {
        let owner = NEXT_BATCH
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("native batch identity exhausted");
        Self {
            owner,
            resources: Vec::new(),
            passes: Vec::new(),
            outputs: Vec::new(),
        }
    }
    pub fn buffer(&mut self, bytes: Vec<u8>) -> Result<ResourceId> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(4) || bytes.len() > u32::MAX as usize {
            return Err("invalid native compute buffer size/alignment".into());
        }
        let id = ResourceId {
            owner: self.owner,
            index: self.resources.len(),
        };
        self.resources.push(Resource::Buffer(bytes));
        Ok(id)
    }
    pub fn texture_rgba8(&mut self, size: [u32; 2], bytes: Vec<u8>) -> Result<ResourceId> {
        self.texture(size, 1, false, bytes)
    }
    pub fn texture_array_rgba8(&mut self, size: [u32; 3], bytes: Vec<u8>) -> Result<ResourceId> {
        self.texture([size[0], size[1]], size[2], true, bytes)
    }
    fn texture(
        &mut self,
        size: [u32; 2],
        layers: u32,
        array: bool,
        bytes: Vec<u8>,
    ) -> Result<ResourceId> {
        let texture = Texture::new(size, layers, array, bytes)?;
        let id = ResourceId {
            owner: self.owner,
            index: self.resources.len(),
        };
        self.resources.push(Resource::Texture(texture));
        Ok(id)
    }
    pub fn sampler(&mut self, filter: SamplerFilter) -> Result<ResourceId> {
        let id = ResourceId {
            owner: self.owner,
            index: self.resources.len(),
        };
        self.resources.push(Resource::Sampler(filter));
        Ok(id)
    }
    pub fn size(&self, id: ResourceId) -> Result<usize> {
        if id.owner != self.owner {
            return Err("resource belongs to another compute batch".into());
        }
        self.resources
            .get(id.index)
            .map(|resource| resource.bytes().len())
            .ok_or_else(|| "invalid compute resource".into())
    }
    /// # Safety
    /// The stage encoder must validate data-dependent indices, write ownership
    /// and barrier-uniform control flow for this kernel. This method additionally
    /// rejects invalid handles, binding layouts, physical sizes and launch grids.
    pub unsafe fn dispatch(
        &mut self,
        entry: &str,
        bindings: &[(u32, ResourceId)],
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
            if (binding.kind == BindingKind::Sampler)
                != matches!(self.resources[id.index], Resource::Sampler(_))
            {
                return Err("native compute sampler kind mismatch".into());
            }
            let texture_binding = matches!(
                binding.kind,
                BindingKind::Texture | BindingKind::TextureWrite | BindingKind::TextureArray
            );
            if texture_binding != matches!(self.resources[id.index], Resource::Texture(_)) {
                return Err("native compute resource kind mismatch".into());
            }
            if let Resource::Texture(texture) = &self.resources[id.index]
                && texture.array != (binding.kind == BindingKind::TextureArray)
            {
                return Err("native compute texture view dimension mismatch".into());
            }
            if ordered
                .iter()
                .any(|(other, other_id): &(Binding, ResourceId)| {
                    *other_id == id && (binding.kind.writable() || other.kind.writable())
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
    pub fn readback(&mut self, id: ResourceId) -> Result<usize> {
        self.size(id)?;
        if matches!(self.resources[id.index], Resource::Sampler(_)) {
            return Err("samplers have no byte readback".into());
        }
        if let Some(index) = self.outputs.iter().position(|old| *old == id) {
            return Ok(index);
        }
        let index = self.outputs.len();
        self.outputs.push(id);
        Ok(index)
    }
    pub fn resources(&self) -> &[Resource] {
        &self.resources
    }
    pub fn passes(&self) -> &[Pass] {
        &self.passes
    }
    pub fn outputs(&self) -> &[ResourceId] {
        &self.outputs
    }
}
#[cfg(test)]
#[path = "tests/compute.rs"]
mod tests;
