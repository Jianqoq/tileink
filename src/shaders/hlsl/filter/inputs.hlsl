#include "region.hlsli"
#include "composite.hlsli"
#include "color_space.hlsli"
#include "../shared/blend.hlsli"
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
void filter_blend_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    uint source=unorm_to_rgba8(source_texture.Load(int3(xy,0)));
    uint backdrop=unorm_to_rgba8(aux_texture.Load(int3(xy,0)));
    if(config.linear_rgb==1u) {
        source=filter_premul_srgb_to_linear(source);
        backdrop=filter_premul_srgb_to_linear(backdrop);
    }
    uint result=blend_premul_u8(backdrop,source,config.blend_mode);
    target_texture[xy]=rgba8_to_unorm(config.linear_rgb==1u?filter_premul_linear_to_srgb(result):result);
}

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_composite_inputs_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    uint source=unorm_to_rgba8(source_texture.Load(int3(xy,0)));
    uint backdrop=unorm_to_rgba8(aux_texture.Load(int3(xy,0)));
    if(config.linear_rgb==1u) {
        source=filter_premul_srgb_to_linear(source);
        backdrop=filter_premul_srgb_to_linear(backdrop);
    }
    uint result=filter_composite_pixel(source,backdrop,config.filter_kind,config.matrix_bias);
    target_texture[xy]=rgba8_to_unorm(config.linear_rgb==1u?filter_premul_linear_to_srgb(result):result);
}

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_apply_region_mask(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config,active_tiles,gid,xy)) return;
    uint destination=unorm_to_rgba8(target_texture[xy]);
    uint mask=unorm_to_rgba8(aux_texture.Load(int3(xy,0)));
    target_texture[xy]=float(combine_alpha(destination>>24u,mask>>24u)).xxxx*CHANNEL_SCALE;
}
