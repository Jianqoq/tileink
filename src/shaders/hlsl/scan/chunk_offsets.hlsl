#include "../scene_records.hlsli"
#include "../constants.hlsli"
#include "config.hlsli"
#include "index.hlsli"

ByteAddressBuffer active_indices:register(t6,space0);
ConstantBuffer<ScanConfig> config : register(b0, space0);
ByteAddressBuffer path_records:register(t1,space0);
ByteAddressBuffer scan_chunk_ranges:register(t2,space0);
RWByteAddressBuffer segment_bumps:register(u3,space0);
RWByteAddressBuffer chunk_totals:register(u4,space0);
RWByteAddressBuffer chunk_offsets:register(u5,space0);

[numthreads(SCAN_CHUNK_SIZE,1,1)]
void scan_chunk_offsets(uint3 id:SV_DispatchThreadID) {
    if(id.x>=config.path_count) return;
    uint path=dispatched_index(active_indices, config.incremental, id.x,config.path_base);
    uint start=path_records.Load(path*PATH_RECORD_STRIDE+PATH_SEGMENT_START);
    uint next=start;
    uint2 range=scan_chunk_ranges.Load2(path*SCAN_CHUNK_RANGE_STRIDE);
    for(uint chunk=range.x;chunk<range.y;chunk++) {
        chunk_offsets.Store(chunk*4u,next);
        next+=chunk_totals.Load(chunk*4u);
    }
    segment_bumps.Store(path*4u,next-start);
}
