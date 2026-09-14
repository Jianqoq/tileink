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
void filter_clear_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (filter_position(config, active_tiles, gid, xy)) target_texture[xy] = rgba8_to_unorm(config.clear_color);
}

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_copy_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (filter_position(config, active_tiles, gid, xy)) target_texture[xy] = source_texture.Load(int3(xy,0));
}

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_source_alpha_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (filter_position(config, active_tiles, gid, xy)) target_texture[xy] = float4(0,0,0,source_texture.Load(int3(xy,0)).a);
}

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_tile_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config, active_tiles, gid, xy)) return;
    uint2 origin = uint2(config.rect_x0, config.rect_y0);
    uint2 size = uint2(config.rect_x1, config.rect_y1) - origin;
    if (any(size == 0u)) return;
    uint2 source_xy = origin + (xy + size - origin % size) % size;
    target_texture[xy] = source_texture.Load(int3(source_xy,0));
}

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_offset_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config, active_tiles, gid, xy)) return;
    int2 source_xy = int2(xy) - int2(config.offset_x, config.offset_y);
    float4 pixel = 0;
    if (filter_contains(config, source_xy)) pixel = source_texture.Load(int3(source_xy,0));
    target_texture[xy] = pixel;
}

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_drop_shadow_mask_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config, active_tiles, gid, xy)) return;
    float alpha = source_texture.Load(int3(xy,0)).a;
    if (alpha == 0) return;
    int2 destination = int2(xy) + int2(config.offset_x, config.offset_y);
    if (filter_contains(config, destination)) target_texture[destination] = alpha.xxxx;
}
