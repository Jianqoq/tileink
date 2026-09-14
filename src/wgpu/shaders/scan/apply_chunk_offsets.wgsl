#include "common.wgsl"

@group(0) @binding(1) var<storage, read> scan_chunks: array<GpuScanChunk>;
@group(0) @binding(2) var<storage, read_write> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(3) var<storage, read_write> segment_tile_cursors: array<u32>;
@group(0) @binding(4) var<storage, read_write> chunk_offsets: array<u32>;
@group(0) @binding(5) var<storage, read> active_indices: array<u32>;

@compute @workgroup_size(SCAN_CHUNK_SIZE)
fn scan_apply_chunk_offsets(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let local_chunk = linear_workgroup_index(workgroup_id, num_workgroups);
    // Buffer capacity can contain stale chunks: guard the logical count before lookup.
    if (local_chunk >= config.scan_chunk_count) {
        return;
    }
    let chunk_ix = dispatched_index(local_chunk, config.chunk_base);
    let lane = local_id.x;
    let chunk = scan_chunks[chunk_ix];
    let chunk_len = chunk.len;
    if (lane >= chunk_len) {
        return;
    }

    let ix = chunk.backdrop_offset + lane;
    let base = chunk_offsets[chunk_ix];
    let start = segment_ranges[ix].start + base;
    let end = segment_ranges[ix].end + base;
    segment_ranges[ix].start = start;
    segment_ranges[ix].end = end;
    // Scan chunks partition the records, so this cursor has a single writer.
    segment_tile_cursors[ix] = start;
}

