//! Shared dirty-range policy backed by persistent, queue-ordered native storage.
use crate::native::runtime::{
    Result,
    buffer::Buffer,
    compute::{ComputeBatch, Resource, ResourceId},
};
use std::{cell::Cell, ops::Range, rc::Rc};

#[derive(Default)]
pub(crate) struct CachedBuffer {
    buffer: Option<Buffer>,
    len: usize,
    accepted: Option<Rc<Cell<bool>>>,
}
impl CachedBuffer {
    pub fn upload<T: bytemuck::Pod>(
        &mut self,
        batch: &mut ComputeBatch,
        data: &[T],
        dirty: Option<&[Range<usize>]>,
    ) -> Result<ResourceId> {
        let Some(adapter) = batch.adapter() else {
            return super::resources::upload(batch, data);
        };
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let needed = bytes.len().max(4);
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
        let buffer = self.buffer.as_ref().unwrap();
        // CPU preparation may have advanced before recording/submit failed. Only
        // accepted uploads allow the next dirty delta to depend on prior contents.
        let full = !buffer.state.initialized.get()
            || self
                .accepted
                .as_ref()
                .is_none_or(|accepted| !accepted.get())
            || dirty.is_none();
        let id = if full {
            let mut complete = vec![0; buffer.state.size];
            complete[..bytes.len()].copy_from_slice(bytes);
            batch.import_buffer(buffer, &[(0, &complete)])?
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
        Ok(id)
    }
}
