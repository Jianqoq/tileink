#include "config.hlsli"
#include "region.hlsli"
#include "../constants.hlsli"
#include "../shared/pixel.hlsli"
#include "../shared/composite_stack.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
Texture2D<float4> source_texture : register(t1);
Texture2D<float4> aux_texture : register(t2);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer active_tiles : register(t8);
ByteAddressBuffer paint : register(t10);
ByteAddressBuffer draws : register(t20);
ByteAddressBuffer paths : register(t21);
ByteAddressBuffer backdrops : register(t22);
ByteAddressBuffer ranges : register(t23);
ByteAddressBuffer segments : register(t24);
ByteAddressBuffer layers : register(t25);
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_composite_stack_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;if(!filter_position(config,active_tiles,gid,xy))return;
    uint result=composite_stack_pixel(config,draws,paths,backdrops,ranges,segments,paint,layers,
        unorm_to_rgba8(target_texture[xy]),unorm_to_rgba8(source_texture.Load(int3(xy,0))),
        unorm_to_rgba8(aux_texture.Load(int3(xy,0))),xy,config.mask_enabled!=0u,false);
    target_texture[xy]=rgba8_to_unorm(result);
}
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_composite_blend_stack_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;if(!filter_position(config,active_tiles,gid,xy))return;
    uint result=composite_stack_pixel(config,draws,paths,backdrops,ranges,segments,paint,layers,
        unorm_to_rgba8(target_texture[xy]),unorm_to_rgba8(source_texture.Load(int3(xy,0))),
        unorm_to_rgba8(aux_texture.Load(int3(xy,0))),xy,true,true);
    target_texture[xy]=rgba8_to_unorm(result);
}
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_composite_surface_stack_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;if(!filter_position(config,active_tiles,gid,xy))return;
    int2 source_position=int2(xy)-int2(config.offset_x,config.offset_y);
    if(any(source_position<0)||any(source_position>=int2(config.kernel_columns,config.kernel_rows)))return;
    uint result=composite_stack_pixel(config,draws,paths,backdrops,ranges,segments,paint,layers,
        unorm_to_rgba8(target_texture[xy]),unorm_to_rgba8(source_texture.Load(int3(source_position,0))),
        0u,xy,false,false);
    target_texture[xy]=rgba8_to_unorm(result);
}
