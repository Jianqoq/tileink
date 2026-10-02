use crate::native::runtime::Result;

/// Validated borrowed ranges keep sizing and serialization tied to the same data.
pub(super) struct Upload<'a> {
    updates: &'a [(usize, &'a [u8])],
    pub(super) byte_len: usize,
    pub(super) copy_count: usize,
    #[cfg(any(feature = "dx12", feature = "metal", test))]
    pub(super) destination_size: usize,
}

#[derive(Default)]
pub(super) struct PackedUpdates {
    pub(super) bytes: Vec<u8>,
    pub(super) copies: Vec<[u64; 3]>,
}

impl<'a> Upload<'a> {
    pub(super) fn new(size: usize, updates: &'a [(usize, &'a [u8])]) -> Result<Self> {
        let mut byte_len = 0;
        let mut copy_count = 0;
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
            if copy_count == 0 || offset != end {
                copy_count += 1;
            }
            end = offset + values.len();
            // Sorted disjoint ranges bound their combined length by size.
            byte_len += values.len();
        }
        Ok(Self {
            updates,
            byte_len,
            copy_count,
            #[cfg(any(feature = "dx12", feature = "metal", test))]
            destination_size: size,
        })
    }

    pub(super) fn copies(&self) -> impl Iterator<Item = [u64; 3]> + '_ {
        let mut updates = self.updates.iter().peekable();
        let mut source = 0;
        std::iter::from_fn(move || {
            let &(destination, values) = updates.next()?;
            let mut length = values.len();
            // Coalesce only adjacent ranges. Gaps can contain GPU-owned bytes.
            while updates
                .peek()
                .is_some_and(|&&(offset, _)| offset == destination + length)
            {
                length += updates.next().expect("peeked upload range").1.len();
            }
            let copy = [source as u64, destination as u64, length as u64];
            source += length;
            Some(copy)
        })
    }

    pub(super) fn append_payload(&self, bytes: &mut Vec<u8>) {
        for &(_, values) in self.updates {
            bytes.extend_from_slice(values);
        }
    }

    pub(super) fn into_packed(self) -> PackedUpdates {
        // Size before copying: repeated Vec growth recopied multi-megabyte
        // journals. Scatter writes directly into its final packet.
        let mut bytes = Vec::with_capacity(self.byte_len);
        let mut copies = Vec::with_capacity(self.copy_count);
        copies.extend(self.copies());
        self.append_payload(&mut bytes);
        PackedUpdates { bytes, copies }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_uploads_share_one_copy_but_preserve_holes() {
        let a = [1; 4];
        let b = [2; 8];
        let c = [3; 4];
        let packed = Upload::new(24, &[(0, &a), (4, &b), (16, &c)])
            .unwrap()
            .into_packed();
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
    fn packed_upload_owns_payload_after_source_changes() {
        let mut source = [1, 2, 3, 4];
        let packed = Upload::new(16, &[(4, &source)]).unwrap().into_packed();
        source.fill(9);
        assert_eq!(packed.bytes, [1, 2, 3, 4]);
        assert_eq!(packed.copies, [[0, 4, 4]]);
    }

    #[test]
    fn fragmented_and_adjacent_uploads_match_independent_cpu_patches() {
        let words: Vec<_> = (0..4096u32).map(u32::to_le_bytes).collect();
        let mut offset = 0usize;
        let updates: Vec<_> = words
            .iter()
            .enumerate()
            .map(|(i, word)| {
                let destination = offset;
                offset += if i % 3 == 0 { 12 } else { 4 };
                (destination, word.as_slice())
            })
            .collect();
        let packed = Upload::new(offset + 16, &updates).unwrap().into_packed();
        let mut expected = vec![0xa5; offset + 16];
        for &(destination, bytes) in &updates {
            expected[destination..destination + bytes.len()].copy_from_slice(bytes);
        }
        let mut actual = vec![0xa5; expected.len()];
        for [source, destination, length] in packed.copies {
            actual[destination as usize..(destination + length) as usize]
                .copy_from_slice(&packed.bytes[source as usize..(source + length) as usize]);
        }
        assert_eq!(actual, expected);
        assert_eq!(
            packed.bytes,
            words.into_iter().flatten().collect::<Vec<_>>()
        );
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
            assert!(Upload::new(16, &updates).is_err());
        }
        let empty = Upload::new(16, &[]).unwrap().into_packed();
        assert!(empty.bytes.is_empty() && empty.copies.is_empty());
    }
}
