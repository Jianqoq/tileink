use wgpu::Device;

use crate::wgpu::{
    buffer::{GpuImageBuffer, WgpuBuffer},
    types::tile_seg::TileSegment,
};

/// Buffer-backed resources prepared for a coarse pass.
pub struct CoarseGpuPrepared {
    pub tile_segments_len: u32,
    pub tile_start_count: u32,
    pub width: u32,
    pub height: u32,
    pub width_in_tiles: u32,
    pub height_in_tiles: u32,
}

impl CoarseGpuPrepared {
    pub fn run(&self, encoder: &mut wgpu::CommandEncoder, pipeline: &CoarseGpuPipeline) {}
}

/// Coarse stage wired for typed wgpu buffers instead of arena allocations.
pub struct CoarseGpuPipeline;

impl CoarseGpuPipeline {
    pub fn new(_device: &Device) -> Self {
        Self
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &self,
        _device: &Device,
        binned_segments: &WgpuBuffer<TileSegment>,
        tile_starts: &WgpuBuffer<u32>,
        output: &mut GpuImageBuffer,
        width: u32,
        height: u32,
        width_in_tiles: u32,
        height_in_tiles: u32,
    ) -> CoarseGpuPrepared {
        output.resize_zeroed(width, height);
        CoarseGpuPrepared {
            tile_segments_len: binned_segments.len() as u32,
            tile_start_count: tile_starts.len() as u32,
            width,
            height,
            width_in_tiles,
            height_in_tiles,
        }
    }
}
