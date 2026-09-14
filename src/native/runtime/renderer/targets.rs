use super::super::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use crate::render::{output::RenderTargetId, retained_surfaces::SurfaceAllocation};

/// A logical surface lease. ComputeBatch and then the submitted API frame own
/// its physical allocation, even after a scratch slot is released or replaced.
pub(crate) struct Surface {
    image: ResourceId,
    size: [u32; 2],
    bytes: u64,
}

impl Surface {
    pub(crate) fn allocate(batch: &mut ComputeBatch, size: [u32; 2]) -> Result<Self> {
        if size.contains(&0) || size.iter().any(|&n| n > i32::MAX as u32) {
            return Err("invalid native surface dimensions".into());
        }
        let bytes = (size[0] as usize)
            .checked_mul(size[1] as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("native surface size overflow")?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(bytes)?;
        pixels.resize(bytes, 0);
        Ok(Self {
            image: batch.texture_rgba8(size, pixels)?,
            size,
            bytes: bytes as u64,
        })
    }

    pub(crate) fn image(&self) -> ResourceId {
        self.image
    }
}

impl SurfaceAllocation for Surface {
    fn byte_len(&self) -> u64 {
        self.bytes
    }
}

struct Slot {
    surface: Option<Surface>,
    occupied: bool,
}

/// Scratch reuse is confined to one ordered batch. A new frame creates a new
/// registry; batch-qualified image IDs prevent importing another frame's pixels.
pub(crate) struct Targets {
    main: Surface,
    scratch: Vec<Slot>,
}

impl Targets {
    pub(crate) fn new(batch: &mut ComputeBatch, size: [u32; 2]) -> Result<Self> {
        Ok(Self {
            main: Surface::allocate(batch, size)?,
            scratch: Vec::new(),
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
                .filter(|slot| slot.occupied)
                .and_then(|slot| slot.surface.as_ref())
                .ok_or_else(|| "native scratch target is not occupied".into()),
        }
    }

    pub(crate) fn acquire(&mut self, batch: &mut ComputeBatch) -> Result<RenderTargetId> {
        batch.size(self.main.image)?;
        let index = self
            .scratch
            .iter()
            .position(|slot| !slot.occupied)
            .unwrap_or(self.scratch.len());
        if index == self.scratch.len() {
            let surface = Surface::allocate(batch, self.main.size)?;
            self.scratch.push(Slot {
                surface: Some(surface),
                occupied: true,
            });
        } else {
            let slot = &mut self.scratch[index];
            if slot.surface.is_none() {
                slot.surface = Some(Surface::allocate(batch, self.main.size)?);
            }
            slot.occupied = true;
        }
        Ok(RenderTargetId::Scratch(index))
    }

    fn slot_mut(&mut self, target: RenderTargetId) -> Result<&mut Slot> {
        let RenderTargetId::Scratch(index) = target else {
            return Err("main surface is not a scratch slot".into());
        };
        self.scratch
            .get_mut(index)
            .filter(|slot| slot.occupied)
            .ok_or_else(|| "native scratch target is not occupied".into())
    }

    pub(crate) fn install(
        &mut self,
        batch: &ComputeBatch,
        target: RenderTargetId,
        surface: Surface,
    ) -> Result<()> {
        batch.size(self.main.image)?;
        batch.size(surface.image)?;
        if surface.size != self.main.size {
            return Err("native scratch surface dimensions do not match context".into());
        }
        self.slot_mut(target)?.surface = Some(surface);
        Ok(())
    }

    pub(crate) fn take(&mut self, target: RenderTargetId) -> Result<Surface> {
        let slot = self.slot_mut(target)?;
        let surface = slot
            .surface
            .take()
            .ok_or("native scratch surface missing")?;
        slot.occupied = false;
        Ok(surface)
    }

    pub(crate) fn release(&mut self, target: RenderTargetId) -> Result<()> {
        self.slot_mut(target)?.occupied = false;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/targets.rs"]
mod tests;
