//! Canvas geometry through scan and backdrop cumsum, without a CPU readback.
//!
//! The shared Canvas/path planner owns the geometry and allocation invariants.
//! This adapter checks their physical extents before recording raw-buffer work.
use super::super::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use super::cumsum::CumsumPlan;
use super::resources::{allocation_size, upload};
#[path = "scene_scan/prepare.rs"]
mod prepare;
use crate::shared::{
    gpu_constants::SCAN_CHUNK_SIZE,
    gpu_plan::{GpuScanChunk, GpuScanChunkRange},
    line::Line,
    line_seg::LineSegment,
    path::PathRecord,
    tile_seg_range::TileSegmentRange,
};
pub use prepare::PreparedScan;

#[derive(Debug)]
pub struct ScanOutput {
    pub paths: ResourceId,
    pub backdrops: ResourceId,
    pub tile_segment_ranges: ResourceId,
    pub segments: ResourceId,
}

#[derive(Default)]
pub(crate) struct ScanBuffers {
    backdrops: super::cached_buffer::CachedBuffer,
    tile_segment_ranges: super::cached_buffer::CachedBuffer,
    counts: super::cached_buffer::CachedBuffer,
    cursors: super::cached_buffer::CachedBuffer,
    bumps: super::cached_buffer::CachedBuffer,
    totals: super::cached_buffer::CachedBuffer,
    offsets: super::cached_buffer::CachedBuffer,
    segments: super::cached_buffer::CachedBuffer,
    cumsum: super::cumsum::CumsumBuffers,
    lines: super::cached_buffer::CachedBuffer,
    paths: super::cached_buffer::CachedBuffer,
    chunks: super::cached_buffer::CachedBuffer,
    ranges: super::cached_buffer::CachedBuffer,
}
pub fn encode_scene(
    batch: &mut ComputeBatch,
    prepared: &PreparedScan<'_>,
    maximum_dimension: u32,
) -> Result<ScanOutput> {
    encode_cached(
        batch,
        prepared,
        maximum_dimension,
        &mut ScanBuffers::default(),
    )
}

pub(crate) fn encode_cached(
    batch: &mut ComputeBatch,
    prepared: &PreparedScan<'_>,
    maximum_dimension: u32,
    buffers: &mut ScanBuffers,
) -> Result<ScanOutput> {
    if maximum_dimension == 0 || maximum_dimension > 65535 {
        return Err("invalid native scan dispatch limit".into());
    }
    let PreparedScan {
        canvas,
        plans,
        lengths,
        dirty,
    } = prepared;
    let line_count = u32::try_from(lengths.line_count)?;
    let path_count = u32::try_from(lengths.path_count)?;
    let chunk_count = u32::try_from(lengths.scan_chunk_count)?;
    let backdrop_count = u32::try_from(lengths.backdrop_len)?;
    let segment_count = u32::try_from(lengths.segment_capacity)?;
    let clear_count = backdrop_count.max(path_count).max(chunk_count);
    for count in [clear_count, line_count, path_count] {
        if count.div_ceil(SCAN_CHUNK_SIZE) > maximum_dimension {
            return Err("native scan dispatch exceeds device grid".into());
        }
    }
    let chunk_grid = grid(chunk_count, maximum_dimension)?;
    let cumsum = plans.cumsum_plan();
    let cumsum = CumsumPlan::new(
        cumsum.chunk_backdrop_offsets.clone(),
        cumsum.chunk_lens.clone(),
        cumsum.row_chunk_starts.clone(),
        cumsum.row_chunk_ends.clone(),
        lengths.backdrop_len,
    )?;
    // Preflight every byte extent before any potentially large allocation.
    for (count, stride) in [
        (lengths.line_count, size_of::<Line>()),
        (lengths.path_count, size_of::<PathRecord>()),
        (lengths.scan_chunk_count, size_of::<GpuScanChunk>()),
        (lengths.path_count, size_of::<GpuScanChunkRange>()),
        (lengths.backdrop_len, size_of::<TileSegmentRange>()),
        (lengths.segment_capacity, size_of::<LineSegment>()),
    ] {
        allocation_size(count, stride)?;
    }
    for path in &canvas.path_records {
        if u64::from(path.data_offset) + u64::from(path.data_len) > u64::from(backdrop_count)
            || u64::from(path.segment_start) + u64::from(path.segment_capacity)
                > u64::from(segment_count)
        {
            return Err("native scan path allocation exceeds prepared capacity".into());
        }
    }
    let config = batch.buffer(
        bytemuck::cast_slice(&[
            clear_count,
            backdrop_count,
            path_count,
            chunk_count,
            line_count,
            segment_count,
            0,
            0,
            0,
            0,
            0,
        ])
        .to_vec(),
    )?;
    let lines = buffers.lines.upload(
        batch,
        &canvas.lines,
        canvas
            .buffer_changes
            .as_ref()
            .map(|changes| changes.lines.as_slice()),
    )?;
    let paths = buffers.paths.upload(
        batch,
        &canvas.path_records,
        canvas
            .buffer_changes
            .as_ref()
            .map(|changes| changes.paths.as_slice()),
    )?;
    let chunks = buffers
        .chunks
        .upload(batch, plans.scan_chunks(), Some(&dirty.scan_chunks))?;
    let chunk_ranges =
        buffers
            .ranges
            .upload(batch, plans.scan_ranges(), Some(&dirty.scan_ranges))?;
    let active = upload(batch, &[0u32])?;
    let backdrops = buffers
        .backdrops
        .scratch(batch, lengths.backdrop_len, size_of::<i32>())?;
    let tile_segment_ranges = buffers.tile_segment_ranges.scratch(
        batch,
        lengths.backdrop_len,
        size_of::<TileSegmentRange>(),
    )?;
    let counts = buffers
        .counts
        .scratch(batch, lengths.backdrop_len, size_of::<u32>())?;
    let cursors = buffers
        .cursors
        .scratch(batch, lengths.backdrop_len, size_of::<u32>())?;
    let bumps = buffers
        .bumps
        .scratch(batch, lengths.path_count, size_of::<u32>())?;
    let totals = buffers
        .totals
        .scratch(batch, lengths.scan_chunk_count, size_of::<u32>())?;
    let offsets = buffers
        .offsets
        .scratch(batch, lengths.scan_chunk_count, size_of::<u32>())?;
    let segments =
        buffers
            .segments
            .scratch(batch, lengths.segment_capacity, size_of::<LineSegment>())?;
    // SAFETY: Canvas builds bounded path/line records and disjoint path work
    // allocations. PersistentPathPlans partitions those allocations into bounded
    // chunks. Their extents are checked above. The ordered clear/count/prefix/
    // offset/apply/emit chain initializes every value consumed by its successor;
    // shaders guard dispatch tails. This is full-scene scan, not sparse replay.
    unsafe {
        if clear_count != 0 {
            batch.dispatch(
                "scan_clear",
                &[
                    (0, config),
                    (1, backdrops),
                    (2, tile_segment_ranges),
                    (3, counts),
                    (4, cursors),
                    (5, bumps),
                    (6, totals),
                    (7, offsets),
                    (8, active),
                ],
                [clear_count.div_ceil(SCAN_CHUNK_SIZE), 1, 1],
            )?;
        }
        if line_count != 0 {
            batch.dispatch(
                "scan_count",
                &[
                    (0, config),
                    (1, lines),
                    (2, paths),
                    (3, backdrops),
                    (4, counts),
                    (5, active),
                ],
                [line_count.div_ceil(SCAN_CHUNK_SIZE), 1, 1],
            )?;
        }
        if chunk_count != 0 {
            batch.dispatch(
                "scan_prefix_chunks",
                &[
                    (0, config),
                    (1, chunks),
                    (2, tile_segment_ranges),
                    (3, counts),
                    (4, totals),
                    (5, active),
                ],
                chunk_grid,
            )?;
        }
        if path_count != 0 {
            batch.dispatch(
                "scan_chunk_offsets",
                &[
                    (0, config),
                    (1, paths),
                    (2, chunk_ranges),
                    (3, bumps),
                    (4, totals),
                    (5, offsets),
                    (6, active),
                ],
                [path_count.div_ceil(SCAN_CHUNK_SIZE), 1, 1],
            )?;
        }
        if chunk_count != 0 {
            batch.dispatch(
                "scan_apply_chunk_offsets",
                &[
                    (0, config),
                    (1, chunks),
                    (2, tile_segment_ranges),
                    (3, cursors),
                    (4, offsets),
                    (5, active),
                ],
                chunk_grid,
            )?;
        }
        if line_count != 0 && segment_count != 0 {
            batch.dispatch(
                "scan_emit",
                &[
                    (0, config),
                    (1, lines),
                    (2, paths),
                    (3, cursors),
                    (4, segments),
                    (5, active),
                ],
                [line_count.div_ceil(SCAN_CHUNK_SIZE), 1, 1],
            )?;
        }
    }
    cumsum.encode_cached(
        batch,
        backdrops,
        maximum_dimension,
        &mut buffers.cumsum,
        Some(dirty),
    )?;
    Ok(ScanOutput {
        paths,
        backdrops,
        tile_segment_ranges,
        segments,
    })
}

fn grid(count: u32, limit: u32) -> Result<[u32; 3]> {
    let x = count.min(limit).max(1);
    let y = count.div_ceil(x).max(1);
    if y > limit {
        return Err("native scan chunks exceed device grid".into());
    }
    Ok([x, y, 1])
}

#[cfg(test)]
#[path = "../tests/scene_scan.rs"]
mod tests;
