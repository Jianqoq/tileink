use super::{Result, compute, frame};
use windows::{Win32::Graphics::Direct3D12::*, core::Interface};
#[derive(Clone)]
pub enum Work {
    Probes(Vec<frame::Frame>),
    Compute(compute::Frame),
}
impl Work {
    pub fn lists(&self) -> Result<Vec<Option<ID3D12CommandList>>> {
        match self {
            Self::Probes(frames) => frames
                .iter()
                .map(|f| f.list.cast().map(Some).map_err(Into::into))
                .collect(),
            Self::Compute(frame) => Ok(vec![Some(frame.list.cast()?)]),
        }
    }
    pub fn readback(&self) -> Result<Vec<Vec<u8>>> {
        match self {
            Self::Probes(frames) => frames.iter().map(frame::Frame::readback).collect(),
            Self::Compute(frame) => frame.readback(),
        }
    }
}
