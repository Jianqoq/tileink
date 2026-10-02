use super::{ComputeBatch, ResourceId, Result, packing::Upload};
use crate::shared::gpu_constants::{RANGE_SCATTER_DESCRIPTOR_WORDS, RANGE_SCATTER_HEADER_WORDS};

// Metal and DX12 encode a host copy call per fragment. Small journals still
// use copies; larger journals amortize one scatter dispatch. Direct packet
// construction avoids the extra payload allocation/copy of the old DX12 path.
const MIN_SCATTER_COPIES: usize = 16;
const MAX_SCATTER_COPIES: usize = 65_535;

pub(super) struct Packet {
    bytes: Vec<u8>,
    groups: u32,
}

pub(super) fn packet(upload: &Upload) -> Option<Packet> {
    let count = upload.copy_count;
    if !(MIN_SCATTER_COPIES..=MAX_SCATTER_COPIES).contains(&count)
        || upload.destination_size > u32::MAX as usize
    {
        return None;
    }
    // This is the existing range_scatter wire format: four header words,
    // four words per descriptor, then tightly packed source words.
    let payload_words =
        RANGE_SCATTER_HEADER_WORDS as usize + count * RANGE_SCATTER_DESCRIPTOR_WORDS as usize;
    let size = (payload_words * 4).checked_add(upload.byte_len)?;
    if size > u32::MAX as usize {
        return None;
    }
    let mut bytes = Vec::with_capacity(size);
    for word in [payload_words as u32, count as u32, 0, 0] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for [source, destination, length] in upload.copies() {
        for word in [destination / 4, source / 4, length / 4, 0] {
            bytes.extend_from_slice(&(word as u32).to_le_bytes());
        }
    }
    upload.append_payload(&mut bytes);
    Some(Packet {
        bytes,
        groups: count as u32,
    })
}

pub(super) fn record(
    batch: &mut ComputeBatch,
    destination: ResourceId,
    packet: Packet,
) -> Result<()> {
    let source = batch.buffer(packet.bytes)?;
    // SAFETY: Upload validates sorted, disjoint, aligned destinations;
    // packet bounds all byte addresses and creates a complete source descriptor
    // and payload for each dispatched group. No group writes a gap.
    unsafe {
        batch.dispatch(
            "range_scatter",
            &[(0, destination), (1, source)],
            [packet.groups, 1, 1],
        )?;
    }
    // Keep the range-upload-before-compute contract. The destination has just
    // been imported once; earlier commands cannot refer to its resource ID.
    // Different imported buffers have disjoint allocations, so their scatters
    // may run in either order, but must precede the recorded rendering work.
    let upload = batch.commands.pop().expect("scatter dispatch was recorded");
    batch.commands.insert(0, upload);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scatter_packet_preserves_descriptors_and_payload() {
        let data = [1, 2, 3, 4];
        let updates: Vec<_> = (0..32).map(|i| (i * 12, data.as_slice())).collect();
        let upload = Upload::new(384, &updates).unwrap();
        let packet = packet(&upload).unwrap();
        let (words, remainder) = packet.bytes.as_chunks::<4>();
        assert!(remainder.is_empty());
        let words: Vec<_> = words.iter().copied().map(u32::from_le_bytes).collect();
        assert_eq!(packet.groups, 32);
        let validated =
            crate::native::runtime::program::Scatter::new(packet.bytes.clone(), vec![0; 384])
                .unwrap();
        assert_eq!(validated.workgroups(), packet.groups);
        assert_eq!(&words[..4], &[132, 32, 0, 0]);
        for i in 0..32 {
            assert_eq!(
                &words[4 + i * 4..8 + i * 4],
                &[i as u32 * 3, i as u32, 1, 0]
            );
        }
        assert!(
            words[132..]
                .iter()
                .all(|&word| word == u32::from_le_bytes(data))
        );
    }

    #[test]
    fn later_import_uploads_precede_previously_recorded_commands() {
        let data = [1, 2, 3, 4];
        let updates: Vec<_> = (0..32).map(|i| (i * 12, data.as_slice())).collect();
        let upload = Upload::new(384, &updates).unwrap();
        let mut batch = ComputeBatch::new();
        let first = batch.buffer(vec![0; 384]).unwrap();
        record(&mut batch, first, packet(&upload).unwrap()).unwrap();
        let second = batch.buffer(vec![0; 384]).unwrap();
        record(&mut batch, second, packet(&upload).unwrap()).unwrap();
        // Uploads imported after command recording still execute first. The two
        // independently owned destinations make reversing their uploads safe.
        assert!(matches!(
            batch.commands.as_slice(),
            [
                crate::native::runtime::compute::Command::Dispatch(1),
                crate::native::runtime::compute::Command::Dispatch(0),
            ]
        ));
    }

    #[test]
    fn small_or_unaddressable_uploads_keep_the_copy_path() {
        let data = [0; 4];
        let updates = [(0, data.as_slice())];
        assert!(packet(&Upload::new(4, &updates).unwrap()).is_none());
        let updates: Vec<_> = (0..16).map(|i| (i * 12, data.as_slice())).collect();
        if let Some(size) = (u32::MAX as usize).checked_add(1) {
            assert!(packet(&Upload::new(size, &updates).unwrap()).is_none());
        }
    }

    #[test]
    fn scatter_thresholds_use_coalesced_copy_count() {
        let data = [0; 4];
        for count in [15, 16, 65_535, 65_536] {
            let updates: Vec<_> = (0..count).map(|i| (i * 8, data.as_slice())).collect();
            let upload = Upload::new(count * 8, &updates).unwrap();
            assert_eq!(packet(&upload).is_some(), (16..=65_535).contains(&count));
        }
        let updates: Vec<_> = (0..32).map(|i| (i * 4, data.as_slice())).collect();
        assert!(packet(&Upload::new(128, &updates).unwrap()).is_none());
    }
}
