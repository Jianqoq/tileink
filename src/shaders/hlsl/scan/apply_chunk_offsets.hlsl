#include "../dispatch.hlsli"
#include "../scene_records.hlsli"
#include "../constants.hlsli"
#include "config.hlsli"
#include "index.hlsli"

ConstantBuffer<DispatchGrid> dispatch_grid : register(b31, space0);
ByteAddressBuffer active_indices:register(t5,space0);
ConstantBuffer<ScanConfig> config : register(b0, space0);
ByteAddressBuffer scan_chunks:register(t1,space0);
RWByteAddressBuffer segment_ranges:register(u2,space0);
RWByteAddressBuffer segment_tile_cursors:register(u3,space0);
RWByteAddressBuffer chunk_offsets:register(u4,space0);

[numthreads(SCAN_CHUNK_SIZE,1,1)]
void scan_apply_chunk_offsets(uint3 group:SV_GroupID,uint3 local:SV_GroupThreadID) {
    uint local_chunk=linear_group(group, uint2(dispatch_grid.x, dispatch_grid.y));
    if(local_chunk>=config.scan_chunk_count) return;
    uint chunk_index=dispatched_index(active_indices, config.incremental, local_chunk,config.chunk_base);
    uint4 chunk=scan_chunks.Load4(chunk_index*SCAN_CHUNK_STRIDE);
    if(local.x>=chunk.w) return;
    uint index=chunk.y+local.x;
    uint base=chunk_offsets.Load(chunk_index*4u);
    uint2 range=segment_ranges.Load2(index*TILE_SEGMENT_RANGE_STRIDE)+base;
    segment_ranges.Store2(index*TILE_SEGMENT_RANGE_STRIDE,range);
    segment_tile_cursors.Store(index*4u,range.x);
}
