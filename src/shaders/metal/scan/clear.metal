#include <metal_stdlib>
using namespace metal;
#include "config.metal"

kernel void scan_clear(constant ScanConfig& config [[buffer(0)]],
    device uint* backdrops [[buffer(1)]],device packed_uint2* ranges [[buffer(2)]],
    device uint* counts [[buffer(3)]],device uint* cursors [[buffer(4)]],
    device uint* bumps [[buffer(5)]],device uint* totals [[buffer(6)]],
    device uint* offsets [[buffer(7)]],const device uint* indices [[buffer(8)]],
    uint index [[thread_position_in_grid]]) {
    if(index>=config.clear_len) return;
    if(index<config.backdrop_len) {
        uint target=scan_index(indices,config.incremental,index,config.backdrop_base);
        backdrops[target]=0;ranges[target]=packed_uint2(0);counts[target]=0;cursors[target]=0;
    }
    if(index<config.path_count) bumps[scan_index(indices,config.incremental,index,config.path_base)]=0;
    if(index<config.scan_chunk_count) {
        uint chunk=scan_index(indices,config.incremental,index,config.chunk_base);
        totals[chunk]=0;offsets[chunk]=0;
    }
}
