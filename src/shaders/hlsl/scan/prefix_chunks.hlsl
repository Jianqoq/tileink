#include "../scene_records.hlsli"
#include "../constants.hlsli"
ByteAddressBuffer active_indices:register(t5,space0);
#include "common.hlsli"
ByteAddressBuffer scan_chunks:register(t1,space0);
RWByteAddressBuffer segment_ranges:register(u2,space0);
ByteAddressBuffer segment_tile_counts:register(t3,space0);
RWByteAddressBuffer chunk_totals:register(u4,space0);
groupshared uint scratch[SCAN_CHUNK_SIZE];

[numthreads(SCAN_CHUNK_SIZE,1,1)]
void scan_prefix_chunks(uint3 group:SV_GroupID,uint3 local:SV_GroupThreadID) {
    uint local_chunk=linear_group(group);
    // Raw native buffers cannot inherit WGSL's robust out-of-range reads.
    if (local_chunk>=config.scan_chunk_count) return;
    uint index=dispatched_index(local_chunk,config.chunk_base);
    uint4 chunk=scan_chunks.Load4(index*SCAN_CHUNK_STRIDE);
    uint lane=local.x;
    uint count=0u;
    if(lane<chunk.w) count=segment_tile_counts.Load((chunk.y+lane)*4u);
    scratch[lane]=count;GroupMemoryBarrierWithGroupSync();
    for(uint step=1u;step<SCAN_CHUNK_SIZE;step*=2u) {
        uint add=0u;if(lane>=step) add=scratch[lane-step];
        GroupMemoryBarrierWithGroupSync();
        if(lane>=step) scratch[lane]+=add;
        GroupMemoryBarrierWithGroupSync();
    }
    if(lane<chunk.w) {
        uint inclusive=scratch[lane];
        segment_ranges.Store2((chunk.y+lane)*TILE_SEGMENT_RANGE_STRIDE,uint2(inclusive-count,inclusive));
    }
    if(lane+1u==chunk.w) chunk_totals.Store(index*4u,scratch[lane]);
}
