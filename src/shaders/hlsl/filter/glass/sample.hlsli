#ifndef TILEINK_FILTER_GLASS_SAMPLE_HLSLI
#define TILEINK_FILTER_GLASS_SAMPLE_HLSLI
#include "constants.hlsli"
#include "../config.hlsli"
#include "../sample.hlsli"
#include "../../shared/pixel.hlsli"
float glass_lerp(float a,float b,float t) { return mad(b-a,t,a); }
float4 liquid_glass_pixel_straight_rgba(uint px) {
    float a = float((px >> 24u) & 255u) / 255.0;
    float r = float(px & 255u) / 255.0;
    float g = float((px >> 8u) & 255u) / 255.0;
    float b = float((px >> 16u) & 255u) / 255.0;
    if (a > LIQUID_GLASS_EPSILON) {
        r = r / a;
        g = g / a;
        b = b / a;
    }
    return float4(r, g, b, a);
}

float4 liquid_glass_premul_to_straight_rgba(float4 premul) {
    float4 rgba = premul;
    if (rgba.a > LIQUID_GLASS_EPSILON) {
        rgba.r = rgba.r / rgba.a;
        rgba.g = rgba.g / rgba.a;
        rgba.b = rgba.b / rgba.a;
    }
    return rgba;
}

float4 liquid_glass_downsampled_blur_straight_rgba_at_full_res(ConstantBuffer<FilterConfig> config, Texture2D<float4> auxiliary, float x, float y) {
    float factor = float(max(config.downsample, 1u));
    float max_x = float(config.source_x1 - 1u);
    float max_y = float(config.source_y1 - 1u);
    float sample_x = clamp((x + 0.5) / factor - 0.5, float(config.source_x0), max_x);
    float sample_y = clamp((y + 0.5) / factor - 0.5, float(config.source_y0), max_y);
    if (config.upsample_filter == 0u) {
        return liquid_glass_pixel_straight_rgba(unorm_to_rgba8(auxiliary.Load(int3(int2(round(float2(sample_x,sample_y))),0))));
    }
    return liquid_glass_premul_to_straight_rgba(filter_sample_premul(auxiliary, uint2(config.width,config.height), float2(sample_x,sample_y)));
}

float4 liquid_glass_sample_downsampled_blur_straight_rgba(ConstantBuffer<FilterConfig> config, Texture2D<float4> auxiliary, float x, float y) {
    if (config.source_x0 >= config.source_x1 || config.source_y0 >= config.source_y1) {
        return float4(0.0,0.0,0.0,0.0);
    }
    float sx = clamp(x, 0.0, float(config.width - 1u));
    float sy = clamp(y, 0.0, float(config.height - 1u));
    return liquid_glass_downsampled_blur_straight_rgba_at_full_res(config, auxiliary, sx, sy);
}

float4 liquid_glass_sample_straight_rgba(ConstantBuffer<FilterConfig> config, Texture2D<float4> source, Texture2D<float4> auxiliary, uint image_kind, float x, float y) {
    if (image_kind == 1u && config.downsample > 1u) {
        return liquid_glass_sample_downsampled_blur_straight_rgba(config, auxiliary, x, y);
    }

    float sx = clamp(x, 0.0, float(config.width - 1u));
    float sy = clamp(y, 0.0, float(config.height - 1u));
    if (image_kind == 1u) {
        return liquid_glass_premul_to_straight_rgba(filter_sample_premul(auxiliary, uint2(config.width,config.height), float2(sx,sy)));
    }
    return liquid_glass_premul_to_straight_rgba(filter_sample_premul(source, uint2(config.width,config.height), float2(sx,sy)));
}

float liquid_glass_sample_straight_channel(ConstantBuffer<FilterConfig> config, Texture2D<float4> source, Texture2D<float4> auxiliary, uint image_kind, float x, float y, uint channel) {
    float4 rgba = liquid_glass_sample_straight_rgba(config, source, auxiliary, image_kind, x, y);
    if (channel == 1u) {
        return rgba.g;
    }
    if (channel == 2u) {
        return rgba.b;
    }
    if (channel == 3u) {
        return rgba.a;
    }
    return rgba.r;
}

float liquid_glass_dispersion_channel(ConstantBuffer<FilterConfig> config, Texture2D<float4> source, Texture2D<float4> auxiliary, float x, float y, float offset_x, float offset_y, float chromatic, uint channel, float blur_mix) {
    float factor = 1.0 - (chromatic - 1.0) * config.liquid_refraction_dispersion;
    // A finite refractive index can overflow its displacement. Zero chromatic
    // scale means the original sample position, including when that displacement
    // is infinite; evaluating infinity * zero would pass NaN to the sampler.
    float2 offset = float2(0.0,0.0);
    if (factor != 0.0) {
        offset = float2(offset_x, offset_y) * factor;
    }
    float sx = x + offset.x;
    float sy = y + offset.y;
    float src = liquid_glass_sample_straight_channel(config, source, auxiliary, 0u, sx, sy, channel);
    float blur = liquid_glass_sample_straight_channel(config, source, auxiliary, 1u, sx, sy, channel);
    return glass_lerp(src, blur, blur_mix);
}

float liquid_glass_pixel_straight_channel(uint px, uint channel) {
    float4 rgba = liquid_glass_pixel_straight_rgba(px);
    if (channel == 1u) {
        return rgba.g;
    }
    if (channel == 2u) {
        return rgba.b;
    }
    if (channel == 3u) {
        return rgba.a;
    }
    return rgba.r;
}

uint liquid_glass_pack_straight_rgba8(float r, float g, float b, float a) {
    float alpha = clamp(a, 0.0, 1.0);
    return pack_premul_rgba8(
        clamp(r, 0.0, 1.0) * alpha,
        clamp(g, 0.0, 1.0) * alpha,
        clamp(b, 0.0, 1.0) * alpha,
        alpha);
}
#endif
