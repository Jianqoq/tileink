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
pub struct Dispatch {
    pub entry: &'static str,
    pub params: Params,
    pub source: Vec<u8>,
    pub destination: Vec<u8>,
}

/// Bounds are checked before recording: raw-buffer shaders have no portable
/// out-of-bounds semantics. Reject malformed work instead of relying on drivers.
pub fn validate_batch(commands: &[Dispatch]) -> super::Result<()> {
    if commands.is_empty() || commands.len() > 4096 {
        return Err("native batch size outside 1..=4096".into());
    }
    for command in commands {
        command.validate()?;
    }
    Ok(())
}
impl Dispatch {
    pub fn validate(&self) -> super::Result<()> {
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
            "layout_words" if p.stride >= 16 && p.stride.is_multiple_of(4) => (p.stride as u64, 16),
            _ => return Err("unknown program or invalid layout stride".into()),
        };
        let end = p.destination_offset as u64
            + if p.count == 0 {
                0
            } else {
                (p.count as u64 - 1) * stride + width
            };
        if end > self.destination.len() as u64 || end > u32::MAX as u64 {
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
                let fixed_origin = (origin as f64 * 65536.0).round();
                let fixed_step = (step as f64 * 65536.0).round();
                let product = fixed_step * p.count.saturating_sub(1) as f64;
                let fixed_last = fixed_origin + product;
                if [fixed_origin, fixed_step, product, fixed_last]
                    .iter()
                    .any(|v| *v < -(i32::MAX as f64) || *v > i32::MAX as f64)
                {
                    return Err("sampling fixed-point coordinate overflow".into());
                }
                p.value[2]
            }
            _ => 0,
        };
        let end = p.source_offset as u64 + source_words as u64 * 4;
        if end > self.source.len() as u64 || end > u32::MAX as u64 {
            return Err("native source out of bounds".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/program.rs"]
mod tests;
