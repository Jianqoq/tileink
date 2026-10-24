#include "region.hlsli"
#include "lighting.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
Texture2D<float4> source_texture : register(t1);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer active_tiles : register(t8);
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_lighting_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (filter_position(config,active_tiles,gid,xy))
        target_texture[xy]=rgba8_to_unorm(filter_lighting_pixel(config,source_texture,xy));
}
