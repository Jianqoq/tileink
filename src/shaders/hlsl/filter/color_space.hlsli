#ifndef TILEINK_FILTER_COLOR_SPACE_HLSLI
#define TILEINK_FILTER_COLOR_SPACE_HLSLI
#include "../shared/pixel.hlsli"
float filter_srgb_to_linear(float value) {
    return value>0.04045 ? pow((value+0.055)/1.055,2.4) : value/12.92;
}
float filter_linear_rgb_to_srgb(float value) {
    // Preserve one fused rounding at half-channel thresholds. Strict SPIR-V
    // math otherwise separates this multiply/subtract from the WGSL/DX12 result.
    return value>0.0031308 ? mad(1.055,pow(value,1.0/2.4),-0.055) : value*12.92;
}
float3 filter_srgb_to_linear_rgb(float3 value) {
    return float3(filter_srgb_to_linear(value.r),filter_srgb_to_linear(value.g),filter_srgb_to_linear(value.b));
}
float3 filter_linear_rgb_to_srgb_rgb(float3 value) {
    return float3(filter_linear_rgb_to_srgb(value.r),filter_linear_rgb_to_srgb(value.g),filter_linear_rgb_to_srgb(value.b));
}
uint filter_premul_srgb_to_linear(uint pixel) {
    float4 value=rgba8_to_unorm(pixel);
    if(value.a==0.0) return 0u;
    float3 rgb=filter_srgb_to_linear_rgb(saturate(value.rgb/value.a))*value.a;
    return pack_premul_rgba8(rgb.r,rgb.g,rgb.b,value.a);
}
uint filter_premul_linear_to_srgb(uint pixel) {
    float4 value=rgba8_to_unorm(pixel);
    if(value.a==0.0) return 0u;
    float3 rgb=filter_linear_rgb_to_srgb_rgb(saturate(value.rgb/value.a))*value.a;
    return pack_premul_rgba8(rgb.r,rgb.g,rgb.b,value.a);
}
#endif
