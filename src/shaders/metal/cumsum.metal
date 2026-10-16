#include <metal_stdlib>
using namespace metal;

struct CumsumConfig { uint row_count; uint chunk_count; uint2 padding; };

// uint arithmetic preserves the signed backdrop's two's-complement sum without
// signed-overflow assumptions. Every barrier is reached by the complete group.
kernel void cumsum_prefix_chunks(constant CumsumConfig& config [[buffer(0)]],
    const device uint* starts [[buffer(1)]], const device uint* lengths [[buffer(2)]],
    device uint* backdrops [[buffer(5)]], device uint* totals [[buffer(6)]],
    constant uint4& grid [[buffer(30)]], uint3 group [[threadgroup_position_in_grid]],
    uint lane [[thread_index_in_threadgroup]]) {
    uint chunk = group.x + grid.x * (group.y + grid.y * group.z);
    if (chunk >= config.chunk_count) return;
    threadgroup uint sums[256];
    uint count = lengths[chunk];
    uint start = starts[chunk];
    uint value = lane < count ? backdrops[start + lane] : 0;
    sums[lane] = value;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for (uint step = 1; step < 256; step *= 2) {
        uint index = (lane + 1) * step * 2 - 1;
        if (index < 256) sums[index] += sums[index - step];
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if (lane == 0) { totals[chunk] = sums[255]; sums[255] = 0; }
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for (uint step = 128; step > 0; step /= 2) {
        uint index = (lane + 1) * step * 2 - 1;
        if (index < 256) {
            uint previous = sums[index - step];
            sums[index - step] = sums[index];
            sums[index] += previous;
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if (lane < count) backdrops[start + lane] = sums[lane] + value;
}

kernel void cumsum_chunk_offsets(constant CumsumConfig& config [[buffer(0)]],
    const device uint* starts [[buffer(3)]], const device uint* ends [[buffer(4)]],
    const device uint* totals [[buffer(6)]], device uint* offsets [[buffer(7)]],
    uint row [[thread_position_in_grid]]) {
    if (row >= config.row_count) return;
    uint sum = 0;
    for (uint chunk = starts[row]; chunk < ends[row]; ++chunk) {
        offsets[chunk] = sum;
        sum += totals[chunk];
    }
}

kernel void cumsum_apply_chunk_offsets(constant CumsumConfig& config [[buffer(0)]],
    const device uint* starts [[buffer(1)]], const device uint* lengths [[buffer(2)]],
    device uint* backdrops [[buffer(5)]], const device uint* offsets [[buffer(7)]],
    constant uint4& grid [[buffer(30)]], uint3 group [[threadgroup_position_in_grid]],
    uint lane [[thread_index_in_threadgroup]]) {
    uint chunk = group.x + grid.x * (group.y + grid.y * group.z);
    if (chunk < config.chunk_count && lane < lengths[chunk])
        backdrops[starts[chunk] + lane] += offsets[chunk];
}
