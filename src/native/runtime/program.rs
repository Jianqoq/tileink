#[path = "program/probe.rs"]
mod probe;
pub use probe::{Params, Probe};

/// Bounds are checked before recording: raw-buffer shaders have no portable
/// out-of-bounds semantics. Reject malformed work instead of relying on drivers.
pub fn validate_batch(commands: &[Dispatch]) -> super::Result<()> {
    if commands.is_empty() || commands.len() > 4096 {
        return Err("native batch size outside 1..=4096".into());
    }
    for command in commands {
        match command {
            Dispatch::Probe(probe) => probe.validate()?,
            Dispatch::Scatter(_) => {}
        }
    }
    Ok(())
}
#[cfg(test)]
#[path = "tests/program.rs"]
mod tests;

#[path = "program/scatter.rs"]
mod scatter;
pub use scatter::Scatter;

/// Keep program-specific data separate: scatter has no uniform block and dispatches
/// one workgroup per range; probe parameters must never determine its launch size.
#[derive(Clone, Debug)]
pub enum Dispatch {
    Probe(Probe),
    Scatter(Scatter),
}
impl From<Probe> for Dispatch {
    fn from(value: Probe) -> Self {
        Self::Probe(value)
    }
}
impl From<Scatter> for Dispatch {
    fn from(value: Scatter) -> Self {
        Self::Scatter(value)
    }
}
impl Dispatch {
    pub fn entry(&self) -> &'static str {
        match self {
            Self::Probe(p) => p.entry,
            Self::Scatter(_) => "range_scatter",
        }
    }
    pub fn source(&self) -> &[u8] {
        match self {
            Self::Probe(p) => &p.source,
            Self::Scatter(s) => s.source(),
        }
    }
    pub fn destination(&self) -> &[u8] {
        match self {
            Self::Probe(p) => &p.destination,
            Self::Scatter(s) => s.destination(),
        }
    }
    pub fn workgroups(&self) -> u32 {
        match self {
            Self::Probe(p) => p.params.count.div_ceil(64).max(1),
            Self::Scatter(s) => s.workgroups(),
        }
    }
    pub fn params(&self) -> Option<&Params> {
        match self {
            Self::Probe(p) => Some(&p.params),
            Self::Scatter(_) => None,
        }
    }
    pub fn texture(&self) -> Option<(u32, &[u8])> {
        match self {
            Self::Probe(p) if p.entry == "sample_words" => {
                let start = p.params.source_offset as usize;
                let width = p.params.value[2];
                Some((width, &p.source[start..start + width as usize * 4]))
            }
            _ => None,
        }
    }
}
