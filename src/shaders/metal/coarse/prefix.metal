#include <metal_stdlib>
using namespace metal;
#include "config.metal"
#include "prefix_scan.metal"

kernel void coarse_prefix_chunks(constant CoarseConfig& c [[buffer(0)]],
    device uint* work [[buffer(7)]], device uint* chunks [[buffer(8)]],
    uint group [[threadgroup_position_in_grid]], uint lane [[thread_index_in_threadgroup]]) {
    if (group >= c.chunk_count) return;
    uint item = group * 256 + lane, base = 0;
    uint2 count(0), total;
    if (item < item_count(c)) {
        base = tile_at(work, c, item) * 6;
        count = uint2(work[base], work[base + 3]);
    }
    threadgroup uint2 scratch[256];
    uint2 start = exclusive_prefix(count, lane, scratch, total);
    if (!lane) { chunks[group * 4] = total.x; chunks[group * 4 + 2] = total.y; }
    if (item < item_count(c)) {
        work[base + 1] = start.x; work[base + 2] = start.x + count.x;
        work[base + 4] = start.y; work[base + 5] = start.y + count.y;
    }
}
kernel void coarse_chunk_offsets(constant CoarseConfig& c [[buffer(0)]],
    device uint* chunks [[buffer(8)]], uint lane [[thread_index_in_threadgroup]]) {
    uint2 carry(0);
    threadgroup uint2 scratch[256];
    for (uint block = 0; block < c.chunk_count; block += 256) {
        uint index = block + lane, base = index * 4;
        uint2 count = index < c.chunk_count ? uint2(chunks[base], chunks[base + 2]) : uint2(0);
        uint2 total;
        uint2 offset = exclusive_prefix(count, lane, scratch, total) + carry;
        if (index < c.chunk_count) { chunks[base + 1] = offset.x; chunks[base + 3] = offset.y; }
        carry += total;
    }
}
kernel void coarse_apply_chunk_offsets(constant CoarseConfig& c [[buffer(0)]],
    device uint* work [[buffer(7)]], const device uint* chunks [[buffer(8)]],
    uint item [[thread_position_in_grid]]) {
    if (item >= item_count(c)) return;
    uint base = tile_at(work, c, item) * 6, chunk = (item / 256) * 4;
    work[base + 1] += chunks[chunk + 1]; work[base + 2] += chunks[chunk + 1];
    work[base + 4] += chunks[chunk + 3]; work[base + 5] += chunks[chunk + 3];
}
