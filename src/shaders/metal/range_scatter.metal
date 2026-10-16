#include <metal_stdlib>
using namespace metal;

// Packed upload ABI: header, four-word range descriptors, then word payload.
// Host validation establishes disjoint destinations before recording this kernel.
kernel void range_scatter(device uint* destination [[buffer(0)]],
    const device uint* source [[buffer(1)]], uint group [[threadgroup_position_in_grid]],
    uint lane [[thread_index_in_threadgroup]]) {
    uint descriptor = 4 + group * 4;
    uint destination_start = source[descriptor];
    uint source_start = source[0] + source[descriptor + 1];
    uint count = source[descriptor + 2];
    for (uint offset = lane; offset < count; offset += 256)
        destination[destination_start + offset] = source[source_start + offset];
}
