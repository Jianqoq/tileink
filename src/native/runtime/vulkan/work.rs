//! Submission leases cover either probe frames or a complete compute batch.
use super::super::Result;
use super::{compute, frame};
use ash::vk;
pub enum Work {
    Probes(Vec<frame::Frame>),
    Compute(Box<compute::Frame>),
}
impl Work {
    pub fn commands(&self) -> Vec<vk::CommandBuffer> {
        match self {
            Self::Probes(v) => v.iter().map(|f| f.command).collect(),
            Self::Compute(f) => vec![f.command],
        }
    }
    pub fn fence(&self) -> Result<vk::Fence> {
        match self {
            Self::Probes(v) => v
                .last()
                .map(|f| f.fence)
                .ok_or_else(|| "empty native submission".into()),
            Self::Compute(f) => Ok(f.fence),
        }
    }
    pub fn readback(&self) -> Result<Vec<Vec<u8>>> {
        match self {
            Self::Probes(v) => v.iter().map(frame::Frame::readback).collect(),
            Self::Compute(f) => f.readback(),
        }
    }
}
