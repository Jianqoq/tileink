#ifndef TILEINK_HLSL_BRUSH_TEXTURE_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_TEXTURE_HLSLI_INCLUDED
#include "constants.hlsli"
#include "../pixel.hlsli"
#include "pattern.hlsli"

uint texture_pattern_pixel(Texture2D<float4> image, uint2 size, uint extend, int2 position) {
    uint2 local = uint2(extend_coord_i32(position.x,size.x,extend),extend_coord_i32(position.y,size.y,extend));
    return unorm_to_rgba8(image.Load(int3(local,0)));
}
uint sample_resource_pattern_texture(Texture2D<float4> image, SamplerState image_sampler,
    float2 position, uint2 size, uint opacity, uint extend, uint sampling) {
    if (any(size == 0u)) return 0u;
    uint color;
    if (sampling == BRUSH_PATTERN_BILINEAR && extend == 0u) {
        uint width, height; image.GetDimensions(width,height);
        float2 uv = clamp(position,0.0,float2(size))/float2(width,height);
        color = unorm_to_rgba8(image.SampleLevel(image_sampler,uv,0.0));
    } else if (sampling == BRUSH_PATTERN_BILINEAR) {
        float2 sample_position = position-0.5;
        float2 base = floor(sample_position), fraction = sample_position-base;
        int2 p = int2(base);
        uint tl=texture_pattern_pixel(image,size,extend,p);
        uint tr=texture_pattern_pixel(image,size,extend,p+int2(1,0));
        uint bl=texture_pattern_pixel(image,size,extend,p+int2(0,1));
        uint br=texture_pattern_pixel(image,size,extend,p+int2(1,1));
        color=lerp_premul_u8(lerp_premul_u8(tl,tr,fraction.x),lerp_premul_u8(bl,br,fraction.x),fraction.y);
    } else {
        color=texture_pattern_pixel(image,size,extend,int2(floor(position)));
    }
    return scale_premul_u8(color,opacity);
}
#endif
