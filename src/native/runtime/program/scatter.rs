//! Packed range uploads use the production WGSL word layout. Validation rules
//! out shader out-of-bounds access and cross-workgroup write races before recording.
use crate::shared::gpu_constants::{RANGE_SCATTER_DESCRIPTOR_WORDS, RANGE_SCATTER_HEADER_WORDS};

#[derive(Clone, Debug)]
pub struct Scatter {
    source: Vec<u8>,
    destination: Vec<u8>,
    workgroups: u32,
}
impl Scatter {
    pub fn new(source: Vec<u8>, destination: Vec<u8>) -> Result<Self, &'static str> {
        if source.len() < RANGE_SCATTER_HEADER_WORDS as usize * 4
            || !source.len().is_multiple_of(4)
            || destination.is_empty()
            || !destination.len().is_multiple_of(4)
            || source.len() > u32::MAX as usize
            || destination.len() > u32::MAX as usize
        {
            return Err("invalid scatter buffer size/alignment");
        }
        let word = |index: usize| {
            u32::from_le_bytes(source[index * 4..index * 4 + 4].try_into().unwrap()) as u64
        };
        let payload = word(0);
        let count = word(1);
        if count > 65_535
            || payload
                != RANGE_SCATTER_HEADER_WORDS as u64 + count * RANGE_SCATTER_DESCRIPTOR_WORDS as u64
            || payload > source.len() as u64 / 4
        {
            return Err("invalid scatter header/descriptor count");
        }
        let mut previous_end = 0;
        for index in 0..count as usize {
            let descriptor = RANGE_SCATTER_HEADER_WORDS as usize
                + index * RANGE_SCATTER_DESCRIPTOR_WORDS as usize;
            let dst = word(descriptor);
            let src = word(descriptor + 1);
            let len = word(descriptor + 2);
            if dst + len > destination.len() as u64 / 4
                || payload + src + len > source.len() as u64 / 4
            {
                return Err("scatter range out of bounds");
            }
            if len != 0 {
                if dst < previous_end {
                    return Err("scatter ranges must be sorted and non-overlapping");
                }
                previous_end = dst + len;
            }
        }
        Ok(Self {
            source,
            destination,
            workgroups: count as u32,
        })
    }
    pub fn workgroups(&self) -> u32 {
        self.workgroups
    }
    pub fn source(&self) -> &[u8] {
        &self.source
    }
    pub fn destination(&self) -> &[u8] {
        &self.destination
    }
}

#[cfg(test)]
mod tests {
    use super::Scatter;

    #[test]
    fn scatter_rejects_truncated_payloads_and_overlapping_destinations() {
        let source = [8u32, 1, 0, 0, 0, 0, 1, 0, 0x01020304];
        let pack = |words: &[u32]| bytemuck::cast_slice(words).to_vec();
        let valid = Scatter::new(pack(&source), vec![0; 4]).unwrap();
        assert_eq!(valid.workgroups, 1);
        assert_eq!(valid.source, pack(&source));
        assert!(Scatter::new(pack(&source[..8]), vec![0; 4]).is_err());
        assert!(Scatter::new(pack(&source), vec![0; 3]).is_err());
        let overlap = [12u32, 2, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 3, 4];
        assert!(Scatter::new(pack(&overlap), vec![0; 8]).is_err());
        let mut adjacent = overlap;
        adjacent[8] = 1;
        assert_eq!(
            Scatter::new(pack(&adjacent), vec![0; 8])
                .unwrap()
                .workgroups,
            2
        );
    }
}
