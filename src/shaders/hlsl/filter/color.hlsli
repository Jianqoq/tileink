#ifndef TILEINK_FILTER_COLOR_HLSLI
#define TILEINK_FILTER_COLOR_HLSLI
#include "config.hlsli"
#include "constants.hlsli"
#include "../shared/pixel.hlsli"

float svg_luminance(float3 value) { return 0.2126*value.r + 0.7152*value.g + 0.0722*value.b; }
float3 filter_lerp(float3 a, float3 b, float t) { return mad(b-a,t,a); }
uint filter_color_pixel(uint pixel, uint kind, float amount) {
    // Invert/sepia operate on byte channels: normalize/unpremultiply/premultiply
    // round trips otherwise move exact halves below the rounding boundary.
    if ((kind == FILTER_INVERT || kind == FILTER_SEPIA) && (pixel>>24u)!=0u) {
        float3 channels=float3(pixel&255u,(pixel>>8u)&255u,(pixel>>16u)&255u);
        float alpha=float(pixel>>24u);
        float3 mapped=alpha-channels;
        if (kind == FILTER_SEPIA) mapped=float3(
            mad(0.393,channels.r,mad(0.769,channels.g,0.189*channels.b)),
            mad(0.349,channels.r,mad(0.686,channels.g,0.168*channels.b)),
            mad(0.272,channels.r,mad(0.534,channels.g,0.131*channels.b)));
        uint3 result=uint3(clamp(mad(mapped-channels,clamp(amount,0.0,1.0),channels),0.0,alpha)+0.5);
        return rgba8_pack(result.r,result.g,result.b,pixel>>24u);
    }
    float4 rgba = rgba8_to_unorm(pixel);
    if (kind == FILTER_OPACITY) {
        rgba *= clamp(amount,0.0,1.0);
    } else if (rgba.a > 0.0) {
        float3 straight = rgba.rgb / rgba.a;
        if (kind == FILTER_BRIGHTNESS) straight *= amount;
        else if (kind == FILTER_CONTRAST) straight = (straight-0.5)*amount+0.5;
        else if (kind == FILTER_GRAYSCALE) straight = filter_lerp(straight,svg_luminance(straight).xxx,clamp(amount,0.0,1.0));
        else if (kind == FILTER_HUE_ROTATE) {
            float angle=amount*0.017453292, co=cos(angle), si=sin(angle);
            float3 result;
            result.r=(0.213+co*0.787-si*0.213)*straight.r + (0.715-co*0.715-si*0.715)*straight.g + (0.072-co*0.072+si*0.928)*straight.b;
            result.g=(0.213-co*0.213+si*0.143)*straight.r + (0.715+co*0.285+si*0.140)*straight.g + (0.072-co*0.072-si*0.283)*straight.b;
            result.b=(0.213-co*0.213-si*0.787)*straight.r + (0.715-co*0.715+si*0.715)*straight.g + (0.072+co*0.928+si*0.072)*straight.b;
            straight=result;
        } else if (kind == FILTER_SATURATE) {
            float luminance=svg_luminance(straight);
            straight=luminance+(straight-luminance)*amount;
        }
        rgba.rgb=clamp(straight,0.0,1.0)*rgba.a;
    }
    return pack_premul_rgba8(rgba.r,rgba.g,rgba.b,rgba.a);
}

float filter_dot4(float4 a, float4 b) { return mad(a.x,b.x,mad(a.y,b.y,mad(a.z,b.z,a.w*b.w))); }

uint filter_color_matrix_pixel(ConstantBuffer<FilterConfig> config, uint pixel) {
    // Keep RGB in premultiplied byte units. Applying a matrix must not lose an
    // exact half channel through normalize/unpremultiply/premultiply round trips.
    float alpha=float(pixel>>24u);
    float3 channels=float3(pixel&255u,(pixel>>8u)&255u,(pixel>>16u)&255u);
    float3 straight=0;
    // Branch before division: selecting a scale still permits reciprocal reassociation.
    if ((pixel>>24u)==255u) straight=channels;
    else if (alpha>0) straight=channels*(255.0/alpha);
    float output_alpha=clamp(filter_dot4(config.matrix_a,float4(straight,alpha))+config.matrix_bias.w*255.0,0.0,255.0);
    float3 result;
    if (alpha>0) {
        float4 scaled=float4(channels,alpha*alpha*CHANNEL_SCALE);
        float3 mapped=float3(filter_dot4(config.matrix_r,scaled),filter_dot4(config.matrix_g,scaled),filter_dot4(config.matrix_b,scaled));
        mapped=mad(config.matrix_bias.rgb,alpha,mapped);
        // Unchanged alpha is an exact identity, not an approximate reciprocal round trip.
        float alpha_scale=output_alpha==alpha ? 1.0 : output_alpha/alpha;
        result=clamp(mapped,0.0,alpha)*alpha_scale;
        // A saturated straight channel is exactly one, so premultiplication is alpha.
        result=float3(mapped.r>=alpha ? output_alpha : result.r,
            mapped.g>=alpha ? output_alpha : result.g, mapped.b>=alpha ? output_alpha : result.b);
    } else result=clamp(config.matrix_bias.rgb,0.0,1.0)*output_alpha;
    uint4 bytes=uint4(clamp(float4(result,output_alpha),0.0,255.0)+0.5);
    return rgba8_pack(bytes.r,bytes.g,bytes.b,bytes.a);
}

#endif
