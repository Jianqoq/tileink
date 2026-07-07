#include "common.wgsl"

@group(0) @binding(1) var<storage, read_write> backdrops: array<atomic<i32>>;
@group(0) @binding(2) var<storage, read_write> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(3) var<storage, read_write> segment_tile_counts: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read_write> segment_tile_cursors: array<atomic<u32>>;
@group(0) @binding(5) var<storage, read_write> segment_bumps: array<u32>;
@group(0) @binding(6) var<storage, read_write> chunk_totals: array<u32>;
@group(0) @binding(7) var<storage, read_write> chunk_offsets: array<u32>;

@compute @workgroup_size(256)
fn scan_clear(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let ix = global_id.x;
    if (ix >= config.clear_len) {
        return;
    }
    if (ix < config.backdrop_len) {
        atomicStore(&backdrops[ix], 0i);
        segment_ranges[ix].start = 0u;
        segment_ranges[ix].end = 0u;
        atomicStore(&segment_tile_counts[ix], 0u);
        atomicStore(&segment_tile_cursors[ix], 0u);
    }
    if (ix < config.path_count) {
        segment_bumps[ix] = 0u;
    }
    if (ix < config.scan_chunk_count) {
        chunk_totals[ix] = 0u;
        chunk_offsets[ix] = 0u;
    }
}

