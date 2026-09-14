//! Shared coarse-kernel selection; GPU adapters only query capabilities and encode work.

use crate::shared::gpu_plan::{COARSE_BIN_TILES, CoarseBinningStats, GpuBufferLengths};

use crate::shared::gpu_constants::COARSE_WORKGROUP_SIZE;

/// Selects the lower-cost coarse kernel from dispatch and candidate-loop work.
///
/// The compact kernels dedicate a 256-lane workgroup to each dirty tile so candidate draws can
/// be reduced in parallel. That is ideal for sparse damage, but a large soft shadow can dirty
/// thousands of tiles containing only one or two draws. Dense bins assign one lane per tile and
/// avoid launching hundreds of mostly idle workgroups while fine rasterization remains compact.
/// Conversely, dense lanes scan candidates serially, so the longest tile list in every bin must
/// be included instead of comparing dispatch counts alone.
pub(crate) fn coarse_binning_costs(
    lengths: GpuBufferLengths,
    stats: CoarseBinningStats,
) -> (u64, u64) {
    let prefix_chunks = |tiles: u32| u64::from(tiles.div_ceil(COARSE_WORKGROUP_SIZE));
    // Both output ranges share one prefix/apply pair and one chunk-offset workgroup.
    let compact_dispatches =
        u64::from(stats.active_tiles) * 2 + prefix_chunks(stats.active_tiles) * 2 + 1;
    let dense_dispatches =
        u64::from(coarse_bin_count(lengths)) * 2 + lengths.coarse_chunk_count as u64 * 2 + 1;
    // Count and emit both traverse the candidate lists. A round represents one 256-lane shader
    // loop: one page for compact, or one serial candidate ordinal for a dense bin.
    (
        compact_dispatches + stats.compact_candidate_rounds * 2,
        dense_dispatches + stats.dense_candidate_rounds * 2,
    )
}

pub(crate) fn prefer_dense_binning(lengths: GpuBufferLengths, stats: CoarseBinningStats) -> bool {
    if stats.active_tiles == 0 {
        return false;
    }
    let (compact, dense) = coarse_binning_costs(lengths, stats);
    dense < compact
}

pub(crate) fn coarse_bin_count(lengths: GpuBufferLengths) -> u32 {
    let bins_x = (lengths.tiles_width as u32).div_ceil(COARSE_BIN_TILES);
    let bins_y = (lengths.tiles_height as u32).div_ceil(COARSE_BIN_TILES);
    bins_x * bins_y
}

#[cfg(test)]
mod tests;
