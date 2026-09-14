#include "constants.hlsli"
#include "dispatch.hlsli"

ConstantBuffer<DispatchGrid> dispatch_grid : register(b31, space0);
struct CumsumConfig { uint row_count; uint chunk_count; uint _pad0; uint _pad1; };
ConstantBuffer<CumsumConfig> config : register(b0, space0);
ByteAddressBuffer chunk_backdrop_offsets : register(t1, space0);
ByteAddressBuffer chunk_lens : register(t2, space0);
ByteAddressBuffer row_chunk_starts : register(t3, space0);
ByteAddressBuffer row_chunk_ends : register(t4, space0);
RWByteAddressBuffer backdrops : register(u5, space0);
RWByteAddressBuffer chunk_totals : register(u6, space0);
RWByteAddressBuffer chunk_offsets : register(u7, space0);
groupshared int scratch[CUMSUM_CHUNK_SIZE];

[numthreads(CUMSUM_CHUNK_SIZE,1,1)]
void cumsum_prefix_chunks(uint3 group:SV_GroupID, uint3 local:SV_GroupThreadID) {
    uint chunk = linear_group(group, uint2(dispatch_grid.x, dispatch_grid.y));
    // WGSL robust access discards padded groups. Native raw buffers require an
    // explicit uniform guard before any memory access or workgroup barrier.
    if (chunk >= config.chunk_count) return;
    uint lane=local.x;
    uint offset=chunk_backdrop_offsets.Load(chunk*4u);
    uint len=chunk_lens.Load(chunk*4u);
    int value=0;
    if (lane < len) value=asint(backdrops.Load((offset+lane)*4u));
    scratch[lane]=value;
    GroupMemoryBarrierWithGroupSync();
    for (uint upsweep_step=1u; upsweep_step<CUMSUM_CHUNK_SIZE; upsweep_step*=2u) {
        uint ix=(lane+1u)*upsweep_step*2u-1u;
        if (ix<CUMSUM_CHUNK_SIZE) scratch[ix]+=scratch[ix-upsweep_step];
        GroupMemoryBarrierWithGroupSync();
    }
    if (lane==0u) {
        chunk_totals.Store(chunk*4u,asuint(scratch[CUMSUM_CHUNK_SIZE-1u]));
        scratch[CUMSUM_CHUNK_SIZE-1u]=0;
    }
    GroupMemoryBarrierWithGroupSync();
    for (uint downsweep_step=CUMSUM_CHUNK_SIZE/2u; downsweep_step!=0u; downsweep_step/=2u) {
        uint ix=(lane+1u)*downsweep_step*2u-1u;
        if (ix<CUMSUM_CHUNK_SIZE) {
            int previous=scratch[ix-downsweep_step];
            scratch[ix-downsweep_step]=scratch[ix];
            scratch[ix]+=previous;
        }
        GroupMemoryBarrierWithGroupSync();
    }
    if (lane<len) backdrops.Store((offset+lane)*4u,asuint(scratch[lane]+value));
}

[numthreads(CUMSUM_CHUNK_SIZE,1,1)]
void cumsum_chunk_offsets(uint3 id:SV_DispatchThreadID) {
    uint row=id.x;
    if (row>=config.row_count) return;
    int carry=0;
    uint end=row_chunk_ends.Load(row*4u);
    for (uint chunk=row_chunk_starts.Load(row*4u);chunk<end;chunk++) {
        chunk_offsets.Store(chunk*4u,asuint(carry));
        carry+=asint(chunk_totals.Load(chunk*4u));
    }
}

[numthreads(CUMSUM_CHUNK_SIZE,1,1)]
void cumsum_apply_chunk_offsets(uint3 group:SV_GroupID,uint3 local:SV_GroupThreadID) {
    uint chunk=linear_group(group, uint2(dispatch_grid.x, dispatch_grid.y));
    if (chunk>=config.chunk_count) return;
    uint lane=local.x;
    if (lane>=chunk_lens.Load(chunk*4u)) return;
    uint ix=chunk_backdrop_offsets.Load(chunk*4u)+lane;
    backdrops.Store(ix*4u,asuint(asint(backdrops.Load(ix*4u))+asint(chunk_offsets.Load(chunk*4u))));
}
