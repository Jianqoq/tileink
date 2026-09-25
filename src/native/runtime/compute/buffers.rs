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
        let packed = pack_updates(buffer.state.size, updates)?;
        #[cfg(feature = "dx12")]
        let (scatter, packed) = match scatter::packet(&packed, buffer.state.size) {
            Some(packet) => (
                Some(packet),
                PackedUpdates {
                    bytes: Vec::new(),
                    copies: Vec::new(),
                },
            ),
            None => (None, packed),
        };
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
        #[cfg(feature = "dx12")]
        if let Some(packet) = scatter {
            scatter::record(self, id, packet)?;
        }
        Ok(id)
    }
}

struct PackedUpdates {
    bytes: Vec<u8>,
    copies: Vec<[u64; 3]>,
}

#[cfg(feature = "dx12")]
#[path = "buffers/scatter.rs"]
mod scatter;

fn pack_updates(size: usize, updates: &[(usize, &[u8])]) -> Result<PackedUpdates> {
    let mut bytes = Vec::new();
    let mut copies: Vec<[u64; 3]> = Vec::new();
    let mut end = 0;
    for &(offset, values) in updates {
        if offset < end
            || !offset.is_multiple_of(4)
            || values.is_empty()
            || !values.len().is_multiple_of(4)
            || offset
                .checked_add(values.len())
                .is_none_or(|end| end > size)
        {
            return Err("invalid or overlapping native buffer upload range".into());
        }
        end = offset + values.len();
        // Dirty journals may retain adjacent entries. Coalesce their packed
        // copies here so DX12 does not record one command per tiny update.
        // Gaps remain separate: uploading their stale bytes would overwrite
        // GPU-owned data between otherwise valid CPU patches.
        if let Some([_, destination, length]) = copies.last_mut()
            && *destination + *length == offset as u64
        {
            *length += values.len() as u64;
        } else {
            copies.push([bytes.len() as u64, offset as u64, values.len() as u64]);
        }
        bytes.extend_from_slice(values);
    }
    Ok(PackedUpdates { bytes, copies })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_uploads_share_one_copy_but_preserve_holes() {
        let a = [1; 4];
        let b = [2; 8];
        let c = [3; 4];
        let packed = pack_updates(24, &[(0, &a), (4, &b), (16, &c)]).unwrap();
        assert_eq!(packed.copies, [[0, 0, 12], [12, 16, 4]]);
        let mut actual = [0xa5; 24];
        for [source, destination, length] in packed.copies {
            actual[destination as usize..(destination + length) as usize]
                .copy_from_slice(&packed.bytes[source as usize..(source + length) as usize]);
        }
        assert_eq!(&actual[..4], &a);
        assert_eq!(&actual[4..12], &b);
        assert_eq!(&actual[12..16], &[0xa5; 4]);
        assert_eq!(&actual[16..20], &c);
        assert_eq!(&actual[20..], &[0xa5; 4]);
    }

    #[test]
    fn packing_rejects_invalid_ranges_before_coalescing() {
        let words = [1; 8];
        for updates in [
            vec![(0, &words[..]), (4, &words[..])],
            vec![(4, &words[..]), (0, &words[..])],
            vec![(2, &words[..])],
            vec![(0, &words[..3])],
            vec![(0, &words[..0])],
            vec![(12, &words[..])],
            vec![(usize::MAX - 3, &words[..])],
        ] {
            assert!(pack_updates(16, &updates).is_err());
        }
        let empty = pack_updates(16, &[]).unwrap();
        assert!(empty.bytes.is_empty() && empty.copies.is_empty());
    }
}
