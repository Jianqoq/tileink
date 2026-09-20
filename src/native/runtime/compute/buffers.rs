use super::{ComputeBatch, Resource, ResourceId};
use crate::native::runtime::{Result, buffer::Buffer};
use std::{cell::Cell, rc::Rc};

pub struct BufferUpload {
    pub buffer: Buffer,
    pub bytes: Vec<u8>,
    /// Packed source offset, device destination offset, and byte count.
    pub copies: Vec<[u64; 3]>,
    pub accepted: Rc<Cell<bool>>,
}
impl ComputeBatch {
    /// Range uploads precede this batch's compute commands. A buffer is imported
    /// once so later CPU recording cannot silently replace an earlier pass's data.
    pub fn import_buffer(
        &mut self,
        buffer: &Buffer,
        updates: &[(usize, &[u8])],
    ) -> Result<ResourceId> {
        if self.resources.iter().any(|resource| matches!(resource, Resource::PersistentBuffer(old) if Rc::ptr_eq(&old.buffer.state, &buffer.state))) {
            return Err("persistent buffer already imported in this batch".into());
        }
        if !(buffer.state.initialized.get()
            || updates.len() == 1 && updates[0].0 == 0 && updates[0].1.len() == buffer.state.size)
        {
            return Err("new persistent buffer requires a complete upload".into());
        }
        let mut bytes = Vec::new();
        let mut copies = Vec::new();
        let mut end = 0;
        for &(offset, values) in updates {
            if offset < end
                || !offset.is_multiple_of(4)
                || values.is_empty()
                || !values.len().is_multiple_of(4)
                || offset
                    .checked_add(values.len())
                    .is_none_or(|end| end > buffer.state.size)
            {
                return Err("invalid or overlapping native buffer upload range".into());
            }
            end = offset + values.len();
            copies.push([bytes.len() as u64, offset as u64, values.len() as u64]);
            bytes.extend_from_slice(values);
        }
        let id = ResourceId {
            owner: self.owner,
            index: self.resources.len(),
        };
        self.resources
            .push(Resource::PersistentBuffer(BufferUpload {
                buffer: buffer.clone(),
                bytes,
                copies,
                accepted: self.accepted.clone(),
            }));
        Ok(id)
    }
}
