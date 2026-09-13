#include "../scene_records.hlsli"
#include "../constants.hlsli"
ByteAddressBuffer active_indices:register(t8,space0);
#include "common.hlsli"
RWByteAddressBuffer backdrops:register(u1,space0);
RWByteAddressBuffer segment_ranges:register(u2,space0);
RWByteAddressBuffer segment_tile_counts:register(u3,space0);
RWByteAddressBuffer segment_tile_cursors:register(u4,space0);
RWByteAddressBuffer segment_bumps:register(u5,space0);
RWByteAddressBuffer chunk_totals:register(u6,space0);
RWByteAddressBuffer chunk_offsets:register(u7,space0);

[numthreads(SCAN_CHUNK_SIZE,1,1)]
void scan_clear(uint3 id:SV_DispatchThreadID) {
    uint index=id.x;
    if(index>=config.clear_len) return;
    if(index<config.backdrop_len) {
        uint backdrop=dispatched_index(index,config.backdrop_base);
        backdrops.Store(backdrop*4u,0u);
        segment_ranges.Store2(backdrop*TILE_SEGMENT_RANGE_STRIDE,uint2(0u,0u));
        segment_tile_counts.Store(backdrop*4u,0u);
        segment_tile_cursors.Store(backdrop*4u,0u);
    }
    if(index<config.path_count) segment_bumps.Store(dispatched_index(index,config.path_base)*4u,0u);
    if(index<config.scan_chunk_count) {
        uint chunk=dispatched_index(index,config.chunk_base);
        chunk_totals.Store(chunk*4u,0u);chunk_offsets.Store(chunk*4u,0u);
    }
}
