#ifndef TILEINK_HLSL_BRUSH_PATTERN_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_PATTERN_HLSLI_INCLUDED
#include "constants.hlsli"
#include "data.hlsli"
#include "../pixel.hlsli"
#include "../pattern_transform.hlsli"

uint extend_coord_i32(int value, uint size, uint extend) {
    if (size <= 1u) return 0u;
    if (extend == BRUSH_EXTEND_REPEAT || extend == BRUSH_EXTEND_REFLECT) {
        // HLSL mixed-sign % is not defined. Use an unsigned magnitude so negative
        // coordinates (including INT_MIN) have portable Euclidean extension.
        uint period = size * (extend == BRUSH_EXTEND_REFLECT ? 2u : 1u);
        uint magnitude = value < 0 ? 0u - asuint(value) : asuint(value);
        uint coord = magnitude % period;
        if (value < 0 && coord != 0u) coord = period - coord;
        return coord < size ? coord : period - coord - 1u;
    }
    return uint(clamp(value, 0, int(size) - 1));
}
uint atlas_pattern_pixel(Texture2DArray<float4> atlas, uint page, uint2 origin, uint2 size, uint extend, int2 position) {
    uint2 local = uint2(extend_coord_i32(position.x, size.x, extend), extend_coord_i32(position.y, size.y, extend));
    return unorm_to_rgba8(atlas.Load(int4(origin + local, page, 0)));
}
uint sample_resource_pattern_atlas(Texture2DArray<float4> atlas, SamplerState image_sampler,
    float2 position, uint page, uint2 origin, uint2 size, uint opacity, uint extend, uint sampling) {
    if (size.x == 0u || size.y == 0u) return 0u;
    uint color;
    if (sampling == BRUSH_PATTERN_BILINEAR && extend == 0u) {
        uint width, height, layers; atlas.GetDimensions(width, height, layers);
        float2 uv = (float2(origin) + clamp(position, 0.0, float2(size))) / float2(width, height);
        color = unorm_to_rgba8(atlas.SampleLevel(image_sampler, float3(uv, float(page)), 0.0));
    } else if (sampling == BRUSH_PATTERN_BILINEAR) {
        float2 sample_position = position - 0.5;
        float2 base = floor(sample_position), fraction = sample_position - base;
        int2 p = int2(base);
        uint tl = atlas_pattern_pixel(atlas, page, origin, size, extend, p);
        uint tr = atlas_pattern_pixel(atlas, page, origin, size, extend, p + int2(1,0));
        uint bl = atlas_pattern_pixel(atlas, page, origin, size, extend, p + int2(0,1));
        uint br = atlas_pattern_pixel(atlas, page, origin, size, extend, p + int2(1,1));
        color = lerp_premul_u8(lerp_premul_u8(tl,tr,fraction.x), lerp_premul_u8(bl,br,fraction.x), fraction.y);
    } else {
        color = atlas_pattern_pixel(atlas, page, origin, size, extend, int2(floor(position)));
    }
    return scale_premul_u8(color, opacity);
}
uint sample_atlas_brush(ByteAddressBuffer paint, uint brush_base, uint data_base,
    Texture2DArray<float4> atlas, SamplerState image_sampler, float x, float y) {
    uint base = data_base + BRUSH_HEADER_WORDS;
    uint2 size = uint2(brush_word(paint,brush_base,data_base+5u), brush_word(paint,brush_base,data_base+6u));
    float tx = pattern_transform_component(brush_param(paint,brush_base,base,0u),brush_param(paint,brush_base,base,2u),brush_param(paint,brush_base,base,4u),x,y) * float(size.x);
    float ty = pattern_transform_component(brush_param(paint,brush_base,base,1u),brush_param(paint,brush_base,base,3u),brush_param(paint,brush_base,base,5u),x,y) * float(size.y);
    return sample_resource_pattern_atlas(atlas,image_sampler,float2(tx,ty),brush_word(paint,brush_base,data_base+4u),
        uint2(brush_word(paint,brush_base,data_base+2u),brush_word(paint,brush_base,data_base+3u)),size,
        brush_word(paint,brush_base,data_base+7u),brush_word(paint,brush_base,data_base+1u),brush_word(paint,brush_base,data_base+8u));
}
#endif // TILEINK_HLSL_BRUSH_PATTERN_HLSLI_INCLUDED