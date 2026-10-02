#include <metal_stdlib>
using namespace metal;
#include "config.metal"

kernel void scan_chunk_offsets(constant ScanConfig& config [[buffer(0)]],
    const device uint* paths [[buffer(1)]],const device packed_uint2* ranges [[buffer(2)]],
    device uint* bumps [[buffer(3)]],const device uint* totals [[buffer(4)]],
    device uint* offsets [[buffer(5)]],const device uint* indices [[buffer(6)]],
    uint index [[thread_position_in_grid]]) {
    if(index>=config.path_count) return;
    uint path=scan_index(indices,config.incremental,index,config.path_base);
    uint start=paths[path*19+10];
    uint next=start;
    uint2 range=uint2(ranges[path]);
    for(uint chunk=range.x;chunk<range.y;++chunk) {offsets[chunk]=next;next+=totals[chunk];}
    bumps[path]=next-start;
}
