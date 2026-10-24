#include "region.hlsli"
#include "resample.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
Texture2D<float4> source_texture : register(t1);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer active_tiles : register(t8);
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_downsample_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (filter_position(config,active_tiles,gid,xy))
        target_texture[xy]=rgba8_to_unorm(filter_downsample_pixel(config,source_texture,xy));
}
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_upsample_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    if (uint(config.rect_x0)>=uint(config.rect_x1) || uint(config.rect_y0)>=uint(config.rect_y1)) return;
    target_texture[xy]=rgba8_to_unorm(filter_upsample_pixel(config,source_texture,xy,uint4(config.rect_x0,config.rect_y0,config.rect_x1,config.rect_y1)));
}
