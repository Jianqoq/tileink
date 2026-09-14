#include "config.hlsli"
#include "../constants.hlsli"
#include "../shared/pixel.hlsli"
#include "region.hlsli"
#include "../shared/layer_geometry.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
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
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_layer_mask_region(uint3 gid : SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    uint alpha=layer_alpha_at(draws,paths,backdrops,ranges,segments,paint,
        config.draw_ix,config.paint_sdf_shadow_base,xy,uint2(config.tiles_width,config.tiles_height));
    target_texture[xy]=rgba8_to_unorm(alpha*0x01010101u);
}
