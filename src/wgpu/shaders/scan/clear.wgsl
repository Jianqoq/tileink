#include "common.wgsl"

@group(0) @binding(1) var<storage, read_write> backdrops: array<i32>;
@group(0) @binding(2) var<storage, read_write> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(3) var<storage, read_write> segment_tile_counts: array<u32>;
@group(0) @binding(4) var<storage, read_write> segment_tile_cursors: array<u32>;
@group(0) @binding(5) var<storage, read_write> segment_bumps: array<u32>;
@group(0) @binding(6) var<storage, read_write> chunk_totals: array<u32>;
@group(0) @binding(7) var<storage, read_write> chunk_offsets: array<u32>;
@group(0) @binding(8) var<storage, read> active_indices: array<u32>;

@compute @workgroup_size(256)
// Dense and active scan plans assign each cleared record to exactly one invocation.
fn scan_clear(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let ix = global_id.x;
    if (ix >= config.clear_len) {
        return;
    }
    if (ix < config.backdrop_len) {
        let backdrop_ix = dispatched_index(ix, config.backdrop_base);
        backdrops[backdrop_ix] = 0i;
        segment_ranges[backdrop_ix].start = 0u;
        segment_ranges[backdrop_ix].end = 0u;
        segment_tile_counts[backdrop_ix] = 0u;
        segment_tile_cursors[backdrop_ix] = 0u;
    }
    if (ix < config.path_count) {
        segment_bumps[dispatched_index(ix, config.path_base)] = 0u;
    }
    if (ix < config.scan_chunk_count) {
        let chunk_ix = dispatched_index(ix, config.chunk_base);
        chunk_totals[chunk_ix] = 0u;
        chunk_offsets[chunk_ix] = 0u;
    }
}

