#include "region.hlsli"
#include "../shared/pixel.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
Texture2D<float4> source_texture : register(t1);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer active_tiles : register(t8);
[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_composite_surface_direct_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    int2 source_position=int2(xy)-int2(config.offset_x,config.offset_y);
    if (any(source_position<0) || any(source_position>=int2(config.kernel_columns,config.kernel_rows))) return;
    uint source=unorm_to_rgba8(source_texture.Load(int3(source_position,0)));
    target_texture[xy]=rgba8_to_unorm(src_over_premul_u8(unorm_to_rgba8(target_texture[xy]),source));
}
