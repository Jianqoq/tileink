#include "config.hlsli"
#include "region.hlsli"
#include "../constants.hlsli"
#include "../shared/pixel.hlsli"
#include "../shared/sdf/base.hlsli"
#include "../shared/sdf/basic.hlsli"
#include "glass/constants.hlsli"
#include "glass/geometry.hlsli"
#include "glass/pixel.hlsli"
ConstantBuffer<FilterConfig> config:register(b0);
Texture2D<float4> source_texture:register(t1);
Texture2D<float4> aux_texture:register(t2);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture:register(u3);
ByteAddressBuffer active_tiles:register(t8);
float glass_distance(ConstantBuffer<FilterConfig> cfg,float2 p) {
    return liquid_glass_round_rect_distance(p.x,p.y,cfg.rect_x0,cfg.rect_y0,cfg.rect_x1,cfg.rect_y1,
        cfg.radius_top_left,cfg.radius_top_right,cfg.radius_bottom_left,cfg.radius_bottom_right);
}
uint glass_color(ConstantBuffer<FilterConfig> cfg,Texture2D<float4> source,Texture2D<float4> auxiliary,uint2 xy,float distance) {
    uint base=unorm_to_rgba8(source.Load(int3(xy,0)));
    float height=float(max(cfg.height,1u)),normalized=distance/height;
    if(normalized>=LIQUID_GLASS_ACTIVE_DISTANCE_NORM)return base;
    float2 p=float2(xy)+0.5;
    return liquid_glass_pixel(cfg,source,auxiliary,base,p.x,p.y,float(xy.x),float(xy.y),distance,normalized,height);
}
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_liquid_glass_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;if(!filter_position(config,active_tiles,gid,xy))return;
    uint color=glass_color(config,source_texture,aux_texture,xy,glass_distance(config,float2(xy)+0.5));
    target_texture[xy]=rgba8_to_unorm(color);
}
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_liquid_glass_rect_composite_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;if(!filter_position(config,active_tiles,gid,xy))return;
    float2 p=float2(xy)+0.5;
    float mask_distance=rect_sdf_distance(p.x,p.y,config.rect_x0,config.rect_y0,config.rect_x1,config.rect_y1,
        config.radius_top_left,config.radius_top_right,config.radius_bottom_left,config.radius_bottom_right);
    uint alpha=coverage_to_u8(sdf_coverage_from_dist(mask_distance));
    if(alpha==0u)return;
    uint color=glass_color(config,source_texture,aux_texture,xy,glass_distance(config,p));
    target_texture[xy]=rgba8_to_unorm(src_over_premul_u8(unorm_to_rgba8(target_texture[xy]),scale_premul_u8(color,alpha)));
}
