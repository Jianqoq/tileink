#include "config.hlsli"
#include "../constants.hlsli"
#include "../shared/pixel.hlsli"
#include "../shared/texture_table_constants.hlsli"
#include "region.hlsli"
#include "../shared/brush/sample.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
Texture2D<float4> aux_texture : register(t2);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer active_tiles : register(t8);
ByteAddressBuffer brush_blob : register(t10);
Texture2DArray<float4> image_resource_atlas : register(t12);
SamplerState image_resource_sampler : register(s13);
Texture2D<float4> image_resource_textures[NATIVE_TEXTURE_TABLE_CAPACITY] : register(t30);

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_flood_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if(!filter_position(config,active_tiles,gid,xy)) return;
    uint color=sample_brush(brush_blob,0u,config.brush_offset,image_resource_atlas,image_resource_sampler,
        image_resource_textures,float(xy.x)+0.5,float(xy.y)+0.5);
    target_texture[xy]=rgba8_to_unorm(color);
}
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_composite_drop_shadow_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if(!filter_position(config,active_tiles,gid,xy)) return;
    uint color=sample_brush(brush_blob,0u,config.brush_offset,image_resource_atlas,image_resource_sampler,
        image_resource_textures,float(xy.x)+0.5,float(xy.y)+0.5);
    uint alpha=unorm_to_rgba8(aux_texture.Load(int3(xy,0)))>>24u;
    uint shadow=scale_premul_u8(color,alpha);
    target_texture[xy]=rgba8_to_unorm(src_over_premul_u8(shadow,unorm_to_rgba8(target_texture[xy])));
}
