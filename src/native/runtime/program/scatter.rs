//! Packed range uploads use the production WGSL word layout. Validation rules
//! out shader out-of-bounds access and cross-workgroup write races before recording.
#[derive(Clone, Debug)]
pub struct Scatter {
    source: Vec<u8>,
    destination: Vec<u8>,
    workgroups: u32,
}
impl Scatter {
    pub fn new(source: Vec<u8>, destination: Vec<u8>) -> Result<Self, &'static str> {
        if source.len() < 16
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
        if count > 65_535 || payload != 4 + count * 4 || payload > source.len() as u64 / 4 {
            return Err("invalid scatter header/descriptor count");
        }
        let mut previous_end = 0;
        for index in 0..count as usize {
            let descriptor = 4 + index * 4;
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
