use super::{ComputeBatch, Resource, ResourceId};
use crate::native::runtime::{Result, buffer::Buffer};
use std::{cell::Cell, rc::Rc};

#[path = "buffers/packing.rs"]
mod packing;
use packing::{PackedUpdates, Upload};

#[cfg(any(feature = "dx12", feature = "metal", test))]
#[path = "buffers/scatter.rs"]
mod scatter;

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
        let upload = Upload::new(buffer.state.size, updates)?;
        #[cfg(any(feature = "dx12", feature = "metal"))]
        let (scatter, packed) = match scatter::packet(&upload) {
            Some(packet) => (Some(packet), PackedUpdates::default()),
            None => (None, upload.into_packed()),
        };
        #[cfg(not(any(feature = "dx12", feature = "metal")))]
        let packed = upload.into_packed();
        let PackedUpdates { bytes, copies } = packed;
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
        #[cfg(any(feature = "dx12", feature = "metal"))]
        if let Some(packet) = scatter {
            scatter::record(self, id, packet)?;
        }
        Ok(id)
    }
}
