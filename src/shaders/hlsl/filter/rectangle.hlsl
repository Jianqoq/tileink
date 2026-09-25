#include "region.hlsli"
#include "resample.hlsli"
#include "../shared/sdf/rect.hlsli"
#include "../shared/pixel.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
Texture2D<float4> source_texture : register(t1);
Texture2D<float4> aux_texture : register(t2);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer active_tiles : register(t8);
uint rectangle_alpha(ConstantBuffer<FilterConfig> settings,uint2 xy) {
    float distance=rect_sdf_distance(float2(xy)+0.5,
        float4(settings.rect_x0,settings.rect_y0,settings.rect_x1,settings.rect_y1),
        float4(settings.radius_top_left,settings.radius_top_right,settings.radius_bottom_left,settings.radius_bottom_right));
    return coverage_to_u8(sdf_coverage_from_dist(distance));
}
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_rect_mask_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    uint alpha=rectangle_alpha(config,xy);
    target_texture[xy]=rgba8_to_unorm(rgba8_pack(alpha,alpha,alpha,alpha));
}
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_composite_direct_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    uint alpha=255u;
    if (config.mask_enabled!=0u) alpha=unorm_to_rgba8(aux_texture.Load(int3(xy,0)))>>24u;
    uint source=scale_premul_u8(unorm_to_rgba8(source_texture.Load(int3(xy,0))),alpha);
    target_texture[xy]=rgba8_to_unorm(src_over_premul_u8(unorm_to_rgba8(target_texture[xy]),source));
}
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_composite_rect_direct_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    uint alpha=rectangle_alpha(config,xy);
    if (alpha==0u) return;
    uint source=scale_premul_u8(unorm_to_rgba8(source_texture.Load(int3(xy,0))),alpha);
    target_texture[xy]=rgba8_to_unorm(src_over_premul_u8(unorm_to_rgba8(target_texture[xy]),source));
}
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_upsample_rect_composite_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    if (config.source_x0>=config.source_x1 || config.source_y0>=config.source_y1) return;
    uint alpha=rectangle_alpha(config,xy);
    if (alpha==0u) return;
    uint source=scale_premul_u8(filter_upsample_pixel(config,source_texture,xy,
        uint4(config.source_x0,config.source_y0,config.source_x1,config.source_y1)),alpha);
    target_texture[xy]=rgba8_to_unorm(src_over_premul_u8(unorm_to_rgba8(target_texture[xy]),source));
}
