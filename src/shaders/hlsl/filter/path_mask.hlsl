#include "region.hlsli"
#include "../shared/pixel.hlsli"
#include "path_mask.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer active_tiles : register(t8);
ByteAddressBuffer path_range_starts : register(t9);
ByteAddressBuffer path_range_ends : register(t10);
ByteAddressBuffer path_p0x : register(t11);
ByteAddressBuffer path_p0y : register(t12);
ByteAddressBuffer path_p1x : register(t13);
ByteAddressBuffer path_p1y : register(t14);
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_path_mask_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    int2 position=int2(xy*PATH_MASK_COORDINATE_SCALE+PATH_MASK_COORDINATE_SCALE/2u);
    int winding=0;
    uint begin=path_range_starts.Load(config.table_index*4u);
    uint end=path_range_ends.Load(config.table_index*4u);
    for(uint line_ix=begin;line_ix<end;++line_ix) {
        int2 start=int2(asint(path_p0x.Load(line_ix*4u)),asint(path_p0y.Load(line_ix*4u)));
        int2 end=int2(asint(path_p1x.Load(line_ix*4u)),asint(path_p1y.Load(line_ix*4u)));
        int delta=0;
        if(start.y<=position.y && end.y>position.y) delta=1;
        if(end.y<=position.y && start.y>position.y) delta=-1;
        if(delta!=0 && path_mask_orientation(start,end,position)==delta) winding+=delta;
    }
    uint alpha=winding!=0 ? 255u : 0u;
    target_texture[xy]=rgba8_to_unorm(rgba8_pack(alpha,alpha,alpha,alpha));
}
