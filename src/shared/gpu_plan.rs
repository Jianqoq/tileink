//! Native GPU planning split by buffer extents, tile membership, capacities
//! and persistent path allocation. Backends consume one shared contract.
use super::gpu_constants::{CUMSUM_CHUNK_SIZE, SCAN_CHUNK_SIZE};
use bytemuck::{Pod, Zeroable};

mod capacity;
mod lengths;
mod path_plans;
mod tile_bins;

pub(crate) use capacity::{
    coarse_glyph_capacity_for_draw, filter_scratch_extra, plan_stack_depths, required_scratch_count,
};
pub(crate) use lengths::{GpuBufferLengths, GpuLengthOverrides, GpuPathPlanCounts};
pub(crate) use path_plans::{
    GpuCumsumPlan, GpuPathPlanDirty, GpuScanChunk, GpuScanChunkRange, PersistentPathPlans,
};
pub(crate) use tile_bins::{ActiveBatches, TileDrawBins};

pub(crate) const COARSE_CHUNK_SIZE: u32 = super::gpu_constants::COARSE_WORKGROUP_SIZE;
pub(crate) const COARSE_BIN_TILES: u32 = 16;
const _: () = assert!(COARSE_CHUNK_SIZE == COARSE_BIN_TILES * COARSE_BIN_TILES);
pub(crate) const FINE_LOCAL_CLIP_DEPTH: usize =
    super::gpu_constants::FINE_LOCAL_CLIP_DEPTH as usize;
pub(crate) const FINE_LOCAL_GROUP_DEPTH: usize =
    super::gpu_constants::FINE_LOCAL_GROUP_DEPTH as usize;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct GpuCanvasConfig {
    pub width: u32,
    pub height: u32,
    pub tiles_width: u32,
    pub tiles_height: u32,
    pub line_count: u32,
    pub path_count: u32,
    pub draw_count: u32,
    pub backdrop_record_count: u32,
    pub backdrop_len: u32,
    pub segment_capacity: u32,
    pub scan_chunk_count: u32,
    pub cumsum_chunk_count: u32,
    pub cumsum_row_count: u32,
    pub coarse_chunk_count: u32,
    pub coarse_ptcl_capacity: u32,
    pub clear_color: u32,
}
