#include <metal_stdlib>
using namespace metal;
#include "config.metal"

kernel void scan_apply_chunk_offsets(constant ScanConfig& config [[buffer(0)]],
    const device uint4* chunks [[buffer(1)]],device packed_uint2* ranges [[buffer(2)]],
    device uint* cursors [[buffer(3)]],const device uint* offsets [[buffer(4)]],
    const device uint* indices [[buffer(5)]],constant uint4& grid [[buffer(30)]],
    uint3 group [[threadgroup_position_in_grid]],uint lane [[thread_index_in_threadgroup]]) {
    uint local_chunk=group.x+grid.x*(group.y+grid.y*group.z);
    if(local_chunk>=config.scan_chunk_count) return;
    uint index=scan_index(indices,config.incremental,local_chunk,config.chunk_base);
    uint4 chunk=chunks[index];
    if(lane>=chunk.w) return;
    uint target=chunk.y+lane;
    uint2 range=uint2(ranges[target])+offsets[index];
    ranges[target]=packed_uint2(range);cursors[target]=range.x;
}
