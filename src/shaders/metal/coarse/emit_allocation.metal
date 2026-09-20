#include <metal_stdlib>
using namespace metal;
#include "config.metal"
#include "prefix_scan.metal"

kernel void coarse_emit_chunk_counts(constant CoarseConfig& c [[buffer(0)]],
    device uint* work [[buffer(7)]], uint tile [[thread_position_in_grid]]) {
    if (tile < c.tile_count) work[tile_emit_base(c, tile)] = (work[tile_draw_base(c, tile) + 1] + 255) / 256;
}
kernel void coarse_emit_prefix_chunks(constant CoarseConfig& c [[buffer(0)]],
    device uint* work [[buffer(7)]], device uint* chunks [[buffer(8)]],
    uint group [[threadgroup_position_in_grid]], uint lane [[thread_index_in_threadgroup]]) {
    if (group >= c.chunk_count) return;
    uint tile = group * 256 + lane;
    uint count = tile < c.tile_count ? work[tile_emit_base(c, tile)] : 0;
    threadgroup uint2 scratch[256];
    uint2 total;
    uint2 start = exclusive_prefix(uint2(count, 0), lane, scratch, total);
    if (!lane) chunks[group * 4] = total.x;
    if (tile < c.tile_count) work[tile_emit_base(c, tile) + 1] = start.x;
}
kernel void coarse_emit_chunk_offsets(constant CoarseConfig& c [[buffer(0)]],
    device uint* chunks [[buffer(8)]]) {
    uint carry = 0;
    for (uint i = 0; i < c.chunk_count; ++i) { chunks[i * 4 + 1] = carry; carry += chunks[i * 4]; }
}
kernel void coarse_emit_apply_chunk_offsets(constant CoarseConfig& c [[buffer(0)]],
    device uint* work [[buffer(7)]], const device uint* chunks [[buffer(8)]],
    uint tile [[thread_position_in_grid]]) {
    if (tile < c.tile_count) work[tile_emit_base(c, tile) + 1] += chunks[(tile / 256) * 4 + 1];
}
kernel void coarse_emit_fill_refs(constant CoarseConfig& c [[buffer(0)]],
    device uint* work [[buffer(7)]], uint tile [[thread_position_in_grid]]) {
    if (tile >= c.tile_count) return;
    uint range = tile_emit_base(c, tile), count = work[range], offset = work[range + 1];
    for (uint i = 0; i < count; ++i) {
        uint reference = offset + i;
        if (reference < c.emit_chunk_capacity) {
            uint base = emit_base(c, reference);
            work[base] = tile; work[base + 1] = i; work[base + 6] = 0;
        }
    }
}
kernel void coarse_emit_chunk_particle_offsets(constant CoarseConfig& c [[buffer(0)]],
    device uint* work [[buffer(7)]], uint tile [[thread_position_in_grid]]) {
    if (tile >= c.tile_count) return;
    uint range = tile_emit_base(c, tile), count = work[range], offset = work[range + 1];
    uint2 carry(0);
    for (uint i = 0; i < count; ++i) {
        uint base = emit_base(c, offset + i);
        work[base + 3] = carry.x; work[base + 5] = carry.y;
        carry += uint2(work[base + 2], work[base + 4]);
    }
}
