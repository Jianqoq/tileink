use super::super::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use crate::render::scratch_slots::ScratchSlots;
use crate::render::{output::RenderTargetId, retained_surfaces::SurfaceAllocation};

/// A logical lease of persistent native storage. The lease, `ComputeBatch` and
/// submitted API frame retain its allocation independently, so retained surfaces
/// can cross batches without relying on a previous batch's resource ID.
pub(crate) struct Surface {
    image: ResourceId,
    texture: crate::native::NativeTexture,
    size: [u32; 2],
    bytes: u64,
}

impl Surface {
    pub(crate) fn allocate(
        batch: &mut ComputeBatch,
        size: [u32; 2],
        clear_color: u32,
    ) -> Result<Self> {
        Self::allocate_inner(batch, size, clear_color, false)
    }

    fn allocate_scratch(batch: &mut ComputeBatch, size: [u32; 2]) -> Result<Self> {
        Self::allocate_inner(batch, size, 0, true)
    }

    #[allow(
        clippy::expect_used,
        reason = "Reusable surfaces are persistent textures owned by the compute batch"
    )]
    fn allocate_inner(
        batch: &mut ComputeBatch,
        size: [u32; 2],
        clear_color: u32,
        scratch: bool,
    ) -> Result<Self> {
        if size.contains(&0) || size.iter().any(|&n| n > i32::MAX as u32) {
            return Err("invalid native surface dimensions".into());
        }
        let image = if scratch {
            batch.reusable_scratch_surface(size)?
        } else {
            batch.reusable_surface(size, clear_color)?
        };
        Ok(Self {
            image,
            texture: batch
                .persistent_texture(image)?
                .expect("pooled native surface")
                .clone(),
            size,
            bytes: batch.size(image)? as u64,
        })
    }

    pub(crate) const fn image(&self) -> ResourceId {
        self.image
    }

    pub(crate) fn image_in(&self, batch: &mut ComputeBatch) -> Result<ResourceId> {
        let adapter = batch.adapter()?;
        if !adapter.same_device(&self.texture.context.adapter) {
            return Err("native surface belongs to another logical device".into());
        }
        batch.import_texture(&self.texture)
    }
}

impl SurfaceAllocation for Surface {
    fn byte_len(&self) -> u64 {
        self.bytes
    }
}

/// Scratch reuse is confined to one ordered batch. A new frame creates a new
/// registry; batch-qualified image IDs prevent importing another frame's pixels.
pub(crate) struct Targets {
    main: Surface,
    scratch: Vec<Option<Surface>>,
    slots: ScratchSlots,
}

impl Targets {
    pub(crate) fn from_image(
        batch: &ComputeBatch,
        image: ResourceId,
        size: [u32; 2],
    ) -> Result<Self> {
        batch.size(image)?;
        let super::super::compute::Resource::Texture(texture) = &batch.resources()[image.index()]
        else {
            return Err("native frame target must be a texture".into());
        };
        if size.contains(&0)
            || texture.size[0] < size[0]
            || texture.size[1] < size[1]
            || texture.array
            || texture.layers != 1
        {
            return Err("native frame target cannot contain the canvas".into());
        }
        Ok(Self {
            main: Surface {
                image,
                texture: texture
                    .persistent
                    .as_ref()
                    .ok_or("native frame target requires a persistent texture")?
                    .clone(),
                size,
                bytes: batch.size(image)? as u64,
            },
            scratch: Vec::new(),
            slots: ScratchSlots::default(),
        })
    }

    pub(crate) fn new(batch: &mut ComputeBatch, size: [u32; 2], clear_color: u32) -> Result<Self> {
        Ok(Self {
            main: Surface::allocate(batch, size, clear_color)?,
            scratch: Vec::new(),
            slots: ScratchSlots::default(),
        })
    }

    pub(crate) const fn size(&self) -> (u32, u32) {
        (self.main.size[0], self.main.size[1])
    }

    pub(crate) fn get(&self, target: RenderTargetId) -> Result<&Surface> {
        match target {
            RenderTargetId::Main => Ok(&self.main),
            RenderTargetId::Scratch(index) => self
                .scratch
                .get(index)
                .filter(|_| self.slots.is_occupied(index))
                .and_then(Option::as_ref)
                .ok_or_else(|| "native scratch target is not occupied".into()),
        }
    }

    pub(crate) fn acquire(&mut self, batch: &mut ComputeBatch) -> Result<RenderTargetId> {
        batch.size(self.main.image)?;
        let index = if let Some(index) = self.slots.acquire() {
            if self.scratch[index].is_none() {
                match Surface::allocate_scratch(batch, self.main.size) {
                    Ok(surface) => self.scratch[index] = Some(surface),
                    Err(error) => {
                        self.slots.release(index);
                        return Err(error);
                    }
                }
            }
            index
        } else {
            let surface = Surface::allocate_scratch(batch, self.main.size)?;
            self.scratch.push(Some(surface));
            self.slots.push_occupied()
        };
        Ok(RenderTargetId::Scratch(index))
    }

    fn slot_index(&self, target: RenderTargetId) -> Result<usize> {
        let RenderTargetId::Scratch(index) = target else {
            return Err("main surface is not a scratch slot".into());
        };
        self.slots
            .is_occupied(index)
            .then_some(index)
            .ok_or_else(|| "native scratch target is not occupied".into())
    }

    pub(crate) fn install(
        &mut self,
        batch: &mut ComputeBatch,
        target: RenderTargetId,
        mut surface: Surface,
    ) -> Result<()> {
        batch.size(self.main.image)?;
        if surface.size != self.main.size {
            return Err("native scratch surface dimensions do not match context".into());
        }
        let index = self.slot_index(target)?;
        surface.image = surface.image_in(batch)?;
        self.scratch[index] = Some(surface);
        Ok(())
    }

    pub(crate) fn take(&mut self, target: RenderTargetId) -> Result<Surface> {
        let index = self.slot_index(target)?;
        let surface = self.scratch[index]
            .take()
            .ok_or("native scratch surface missing")?;
        self.slots.release(index);
        Ok(surface)
    }

    pub(crate) fn release(&mut self, target: RenderTargetId) -> Result<()> {
        let index = self.slot_index(target)?;
        self.slots.release(index);
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/targets.rs"]
mod tests;
