#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Params {
    pub count: u32,
    pub source_offset: u32,
    pub destination_offset: u32,
    pub stride: u32,
    pub value: [u32; 4],
}

#[derive(Clone, Debug)]
pub struct Probe {
    pub entry: &'static str,
    pub params: Params,
    pub source: Vec<u8>,
    pub destination: Vec<u8>,
}

impl Probe {
    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        let p = self.params;
        if self.source.is_empty()
            || self.destination.is_empty()
            || !self.source.len().is_multiple_of(4)
            || !self.destination.len().is_multiple_of(4)
            || !p.source_offset.is_multiple_of(4)
            || !p.destination_offset.is_multiple_of(4)
            || p.count > 65_535 * 64
        {
            return Err("invalid native dispatch dimensions/alignment".into());
        }
        let (stride, width) = match self.entry {
            "clear_words" | "copy_words" | "sample_words" => (4u64, 4u64),
            "layout_words" if p.stride >= 16 && p.stride.is_multiple_of(4) => {
                (u64::from(p.stride), 16)
            }
            _ => return Err("unknown program or invalid layout stride".into()),
        };
        let end = u64::from(p.destination_offset)
            + if p.count == 0 {
                0
            } else {
                (u64::from(p.count) - 1) * stride + width
            };
        if end > self.destination.len() as u64 || end > u64::from(u32::MAX) {
            return Err("native destination out of bounds".into());
        }
        let source_words = match self.entry {
            "copy_words" => p.count,
            "sample_words" => {
                let origin = f32::from_bits(p.value[0]);
                let step = f32::from_bits(p.value[1]);
                let last = origin + p.count.saturating_sub(1) as f32 * step;
                if p.value[2] == 0
                    || p.value[2] > 16_384
                    || p.value[3] > 1
                    || !origin.is_finite()
                    || !step.is_finite()
                    || step.abs() > 16_777_216.0
                    || !last.is_finite()
                    || origin.abs() > 16_777_216.0
                    || last.abs() > 16_777_216.0
                {
                    return Err("invalid sampling coordinate/width".into());
                }
                let fixed_origin = (f64::from(origin) * 65536.0).round();
                let fixed_step = (f64::from(step) * 65536.0).round();
                let product = fixed_step * f64::from(p.count.saturating_sub(1));
                let fixed_last = fixed_origin + product;
                if [fixed_origin, fixed_step, product, fixed_last]
                    .iter()
                    .any(|v| *v < -f64::from(i32::MAX) || *v > f64::from(i32::MAX))
                {
                    return Err("sampling fixed-point coordinate overflow".into());
                }
                p.value[2]
            }
            _ => 0,
        };
        let end = u64::from(p.source_offset) + u64::from(source_words) * 4;
        if end > self.source.len() as u64 || end > u64::from(u32::MAX) {
            return Err("native source out of bounds".into());
        }
        Ok(())
    }
}
