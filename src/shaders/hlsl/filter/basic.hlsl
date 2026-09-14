#include "region.hlsli"
#include "constants.hlsli"
#include "color.hlsli"
#include "../shared/pixel.hlsli"
#include "../shared/blend.hlsli"

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

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_source_over_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config, active_tiles, gid, xy)) return;
    uint source = unorm_to_rgba8(source_texture.Load(int3(xy,0)));
    uint destination = unorm_to_rgba8(target_texture[xy]);
    target_texture[xy] = rgba8_to_unorm(blend_premul_u8(destination, source, COMPOSE_SRC_OVER << 8u));
}

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_svg_mask_coverage_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config, active_tiles, gid, xy)) return;
    uint pixel = unorm_to_rgba8(source_texture.Load(int3(xy,0)));
    uint alpha = pixel >> 24u;
    uint mask_alpha = alpha;
    if (config.mask_kind == SVG_MASK_LUMINANCE) {
        uint safe_alpha = max(alpha,1u);
        uint3 straight = (uint3(pixel & 255u, (pixel >> 8u) & 255u, (pixel >> 16u) & 255u) * 255u + safe_alpha/2u) / safe_alpha;
        mask_alpha = ((2126u*straight.r + 7152u*straight.g + 722u*straight.b)*alpha + 1275000u)/2550000u;
    }
    target_texture[xy] = float(mask_alpha).xxxx * CHANNEL_SCALE;
}

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_color_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config, active_tiles, gid, xy)) return;
    uint pixel=unorm_to_rgba8(target_texture[xy]);
    target_texture[xy]=rgba8_to_unorm(filter_color_pixel(pixel,config.filter_kind,config.amount));
}

[numthreads(FILTER_WORKGROUP_SIZE,1,1)]
void filter_color_matrix_region(uint3 gid:SV_DispatchThreadID) {
    uint2 xy;
    if (!filter_position(config, active_tiles, gid, xy)) return;
    uint pixel=unorm_to_rgba8(target_texture[xy]);
    target_texture[xy]=rgba8_to_unorm(filter_color_matrix_pixel(config,pixel));
}
