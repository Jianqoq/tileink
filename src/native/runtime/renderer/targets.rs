use super::super::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use crate::render::scratch_slots::ScratchSlots;
use crate::render::{output::RenderTargetId, retained_surfaces::SurfaceAllocation};

/// A logical surface lease. ComputeBatch and then the submitted API frame own
/// its physical allocation, even after a scratch slot is released or replaced.
pub(crate) struct Surface {
    image: ResourceId,
    persistent: Option<crate::native::NativeTexture>,
    size: [u32; 2],
    bytes: u64,
}

impl Surface {
    pub(crate) fn allocate(
        batch: &mut ComputeBatch,
        size: [u32; 2],
        clear_color: u32,
    ) -> Result<Self> {
        if size.contains(&0) || size.iter().any(|&n| n > i32::MAX as u32) {
            return Err("invalid native surface dimensions".into());
        }
        let bytes = (size[0] as usize)
            .checked_mul(size[1] as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("native surface size overflow")?;
        if let Some(image) = batch.reusable_surface(size, clear_color)? {
            return Ok(Self {
                image,
                persistent: batch.persistent_texture(image)?.cloned(),
                size,
                bytes: bytes as u64,
            });
        }
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(bytes)?;
        pixels.resize(bytes, 0);
        if clear_color != 0 {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.copy_from_slice(&clear_color.to_le_bytes());
            }
        }
        Ok(Self {
            image: batch.texture_rgba8(size, pixels)?,
            persistent: None,
            size,
            bytes: bytes as u64,
        })
    }

    pub(crate) fn image(&self) -> ResourceId {
        self.image
    }

    pub(crate) fn image_in(&self, batch: &mut ComputeBatch) -> Result<ResourceId> {
        if let Some(texture) = &self.persistent {
            batch.import_texture(texture)
        } else {
            batch.size(self.image)?;
            Ok(self.image)
        }
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
        if texture.size != size || texture.array || texture.layers != 1 {
            return Err("native frame target dimensions differ from canvas".into());
        }
        Ok(Self {
            main: Surface {
                image,
                persistent: texture.persistent.clone(),
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

    pub(crate) fn size(&self) -> (u32, u32) {
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
                match Surface::allocate(batch, self.main.size, 0) {
                    Ok(surface) => self.scratch[index] = Some(surface),
                    Err(error) => {
                        self.slots.release(index);
                        return Err(error);
                    }
                }
            }
            index
        } else {
            let surface = Surface::allocate(batch, self.main.size, 0)?;
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
        surface.image = surface.image_in(batch)?;
        if surface.size != self.main.size {
            return Err("native scratch surface dimensions do not match context".into());
        }
        let index = self.slot_index(target)?;
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
