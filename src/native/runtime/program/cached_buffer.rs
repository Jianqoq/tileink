//! Shared dirty-range policy backed by persistent, queue-ordered native storage.
use crate::native::runtime::{
    Result,
    buffer::Buffer,
    compute::{ComputeBatch, Resource, ResourceId},
};
use std::{cell::Cell, ops::Range, rc::Rc};

// Vulkan copy regions and DX12 scatter dispatches have fixed GPU work per
// patch. A dense journal is cheaper as one contiguous transfer even if it
// includes some unchanged bytes.
const COPY_REGION_OVERHEAD_BYTES: usize = 128;

#[derive(Default)]
pub(crate) struct CachedBuffer {
    buffer: Option<Buffer>,
    len: usize,
    accepted: Option<Rc<Cell<bool>>>,
    cached_upload: Vec<u8>,
}

#[cfg(all(test, any(feature = "dx12", feature = "vulkan")))]
#[path = "cached_buffer_tests.rs"]
mod tests;
impl CachedBuffer {
    /// GPU work is initialized once. The caller's scan/coarse/fine passes must
    /// overwrite every value they consume; previous frame outputs are not cleared
    /// or transferred from the CPU again.
    pub fn scratch(
        &mut self,
        batch: &mut ComputeBatch,
        count: usize,
        stride: usize,
    ) -> Result<ResourceId> {
        let size = super::resources::allocation_size(count, stride)?;
        self.patches(batch, size, &[])
    }

    fn reserve(&mut self, batch: &ComputeBatch, needed: usize) -> Result<bool> {
        let Some(adapter) = batch.adapter() else {
            return Ok(false);
        };
        if self
            .buffer
            .as_ref()
            .is_none_or(|buffer| buffer.state.size < needed)
        {
            self.buffer = Some(Buffer::new(
                &adapter,
                needed
                    .checked_next_power_of_two()
                    .ok_or("native buffer capacity overflow")?,
            )?);
        }
        Ok(true)
    }

    /// Upload CPU-owned regions while preserving GPU-owned work between frames.
    /// New or unaccepted storage is fully initialized before applying the patches.
    pub fn patches(
        &mut self,
        batch: &mut ComputeBatch,
        size: usize,
        updates: &[(usize, &[u8])],
    ) -> Result<ResourceId> {
        self.patches_delta(batch, size, updates, updates)
    }

    /// Use the complete snapshot when storage is new or a previous batch was
    /// abandoned; otherwise transfer only the changes relative to the last
    /// accepted upload. Both sets must describe the same current CPU state.
    pub fn patches_delta(
        &mut self,
        batch: &mut ComputeBatch,
        size: usize,
        full_updates: &[(usize, &[u8])],
        dirty_updates: &[(usize, &[u8])],
    ) -> Result<ResourceId> {
        let native = self.reserve(batch, size.max(4))?;
        let full = !native
            || self
                .buffer
                .as_ref()
                .is_none_or(|buffer| !buffer.state.initialized.get())
            || self
                .accepted
                .as_ref()
                .is_none_or(|accepted| !accepted.get());
        let updates = if full { full_updates } else { dirty_updates };
        let mut previous_end = 0;
        for &(offset, bytes) in updates {
            let end = offset
                .checked_add(bytes.len())
                .ok_or("native patch range overflow")?;
            if offset < previous_end
                || end > size
                || !offset.is_multiple_of(4)
                || bytes.is_empty()
                || !bytes.len().is_multiple_of(4)
            {
                return Err("invalid native buffer patch".into());
            }
            previous_end = end;
        }
        let id = if full {
            let capacity = self
                .buffer
                .as_ref()
                .map_or(size.max(4), |buffer| buffer.state.size);
            let mut complete = Vec::new();
            complete.try_reserve_exact(capacity)?;
            complete.resize(capacity, 0);
            for &(offset, bytes) in updates {
                complete[offset..offset + bytes.len()].copy_from_slice(bytes);
            }
            if !native {
                return batch.buffer(complete);
            }
            batch.import_buffer(self.buffer.as_ref().unwrap(), &[(0, &complete)])?
        } else {
            batch.import_buffer(self.buffer.as_ref().unwrap(), updates)?
        };
        let Resource::PersistentBuffer(upload) = &batch.resources()[id.index()] else {
            unreachable!("persistent import")
        };
        self.accepted = Some(upload.accepted.clone());
        self.len = size;
        Ok(id)
    }

    pub fn upload<T: bytemuck::Pod>(
        &mut self,
        batch: &mut ComputeBatch,
        data: &[T],
        dirty: Option<&[Range<usize>]>,
    ) -> Result<ResourceId> {
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let needed = bytes.len().max(4);
        if !self.reserve(batch, needed)? {
            return super::resources::upload(batch, data);
        }
        let buffer = self.buffer.as_ref().unwrap();
        // CPU preparation may have advanced before recording/submit failed. Only
        // accepted uploads allow the next dirty delta to depend on prior contents.
        let reusable = buffer.state.initialized.get()
            && self
                .accepted
                .as_ref()
                .is_some_and(|accepted| accepted.get());
        let unchanged = reusable && dirty.is_none() && bytes == self.cached_upload;
        let id = if unchanged {
            batch.import_buffer(buffer, &[])?
        } else if !reusable || dirty.is_none() {
            let mut complete = vec![0; buffer.state.size];
            complete[..bytes.len()].copy_from_slice(bytes);
            batch.import_buffer(buffer, &[(0, &complete)])?
        } else if dirty.is_some_and(|ranges| {
            !bytes.is_empty()
                && ranges.iter().fold(0usize, |cost, range| {
                    cost.saturating_add(
                        range
                            .len()
                            .saturating_mul(size_of::<T>())
                            .saturating_add(COPY_REGION_OVERHEAD_BYTES),
                    )
                }) >= bytes.len()
        }) {
            batch.import_buffer(buffer, &[(0, bytes)])?
        } else {
            let ranges = crate::render::upload::ranges::changed_ranges(dirty, self.len, data.len());
            let updates: Vec<_> = ranges
                .iter()
                .map(|range| {
                    let start = range.start * size_of::<T>();
                    let end = range.end * size_of::<T>();
                    (start, &bytes[start..end])
                })
                .collect();
            batch.import_buffer(buffer, &updates)?
        };
        let Resource::PersistentBuffer(upload) = &batch.resources()[id.index()] else {
            unreachable!("persistent import")
        };
        self.accepted = Some(upload.accepted.clone());
        self.len = data.len();
        if !unchanged {
            self.cached_upload.clear();
            if dirty.is_none() {
                self.cached_upload.extend_from_slice(bytes);
            }
        }
        Ok(id)
    }
}
