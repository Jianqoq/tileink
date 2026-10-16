#include "region.hlsli"
#include "turbulence.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer turbulence_selectors : register(t5);
ByteAddressBuffer turbulence_gradients : register(t6);
ByteAddressBuffer active_tiles : register(t8);
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_turbulence_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if(filter_position(config,active_tiles,gid,xy)) target_texture[xy]=rgba8_to_unorm(turbulence_pixel(config,turbulence_selectors,turbulence_gradients,float2(xy)));
}
