#include "region.hlsli"
#include "displacement.hlsli"
#include "../shared/pixel.hlsli"
ConstantBuffer<FilterConfig> config : register(b0);
Texture2D<float4> source_texture : register(t1);
Texture2D<float4> aux_texture : register(t2);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target_texture : register(u3);
ByteAddressBuffer active_tiles : register(t8);

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_displacement_map_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    uint pixel=unorm_to_rgba8(aux_texture.Load(int3(xy,0)));
    float dx=filter_displacement_channel(pixel,config.kernel_edge_mode,config.lighting_output_kind!=0u)-0.5;
    float dy=filter_displacement_channel(pixel,config.kernel_preserve_alpha,config.lighting_output_kind!=0u)-0.5;
    // One explicit FMA avoids backend-dependent cancellation at half-pixel boundaries.
    float2 sample_xy=round(mad(float2(dx,dy),float2(config.amount,config.rect_x0),float2(xy)));
    float4 result=0.0;
    // Guard floating coordinates before conversion: finite scales may still overflow
    // during multiplication, and raw native loads must never see out-of-range coordinates.
    if (all(sample_xy>=0.0) && all(sample_xy<float2(config.width,config.height)))
        result=source_texture.Load(int3(int2(sample_xy),0));
    target_texture[xy]=result;
}
