#include "common.wgsl"

@group(0) @binding(3) var<storage, read> scan_chunk_backdrop_offsets: array<u32>;
@group(0) @binding(4) var<storage, read> scan_chunk_lens: array<u32>;
@group(0) @binding(8) var<storage, read_write> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(11) var<storage, read_write> segment_tile_cursors: array<atomic<u32>>;
@group(0) @binding(14) var<storage, read_write> chunk_offsets: array<u32>;

@compute @workgroup_size(256)
fn scan_apply_chunk_offsets(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let chunk_ix = workgroup_id.x;
    let lane = local_id.x;
    let chunk_len = scan_chunk_lens[chunk_ix];
    if (lane >= chunk_len) {
        return;
    }

    let ix = scan_chunk_backdrop_offsets[chunk_ix] + lane;
    let base = chunk_offsets[chunk_ix];
    let start = segment_ranges[ix].start + base;
    let end = segment_ranges[ix].end + base;
    segment_ranges[ix].start = start;
    segment_ranges[ix].end = end;
    atomicStore(&segment_tile_cursors[ix], start);
}

