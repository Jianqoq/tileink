#include <metal_stdlib>
using namespace metal;
#include "config.metal"

kernel void scan_prefix_chunks(constant ScanConfig& config [[buffer(0)]],
    const device uint4* chunks [[buffer(1)]],device packed_uint2* ranges [[buffer(2)]],
    const device uint* counts [[buffer(3)]],device uint* totals [[buffer(4)]],
    const device uint* indices [[buffer(5)]],constant uint4& grid [[buffer(30)]],
    uint3 group [[threadgroup_position_in_grid]],uint lane [[thread_index_in_threadgroup]]) {
    uint local_chunk=group.x+grid.x*(group.y+grid.y*group.z);
    if(local_chunk>=config.scan_chunk_count) return;
    uint index=scan_index(indices,config.incremental,local_chunk,config.chunk_base);
    uint4 chunk=chunks[index];
    uint value=lane<chunk.w?counts[chunk.y+lane]:0;
    threadgroup uint sums[256];
    sums[lane]=value;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for(uint step=1;step<256;step*=2) {
        uint previous=lane>=step?sums[lane-step]:0;
        threadgroup_barrier(mem_flags::mem_threadgroup);
        sums[lane]+=previous;
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if(lane<chunk.w) ranges[chunk.y+lane]=packed_uint2(sums[lane]-value,sums[lane]);
    if(lane+1==chunk.w) totals[index]=sums[lane];
}
