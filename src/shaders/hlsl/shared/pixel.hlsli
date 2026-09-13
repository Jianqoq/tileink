#ifndef TILEINK_HLSL_SHARED_PIXEL_HLSLI_INCLUDED
#define TILEINK_HLSL_SHARED_PIXEL_HLSLI_INCLUDED

static const uint CHANNEL_MAX = 255u;
static const float CHANNEL_SCALE = 1.0 / 255.0;

uint coverage_to_u8(float coverage) { return uint(clamp(coverage, 0.0, 1.0) * 255.0 + 0.5); }
uint coverage_to_alpha(float value, uint fill_rule) {
    float alpha = min(abs(value), 1.0);
    if (fill_rule == 1u) alpha = abs(value - 2.0 * round(0.5 * value));
    return coverage_to_u8(alpha);
}
float signum_f32(float value) { return value < 0.0 ? -1.0 : 1.0; }
uint rgba8_pack(uint r, uint g, uint b, uint a) { return r | (g << 8u) | (b << 16u) | (a << 24u); }
uint unorm_to_rgba8(float4 pixel) {
    uint4 channels = uint4(clamp(pixel * 255.0 + 0.5, 0.0, 255.0));
    return rgba8_pack(channels.x, channels.y, channels.z, channels.w);
}
float4 rgba8_to_unorm(uint pixel) {
    return float4(pixel & CHANNEL_MAX, (pixel >> 8u) & CHANNEL_MAX, (pixel >> 16u) & CHANNEL_MAX, pixel >> 24u) * CHANNEL_SCALE;
}
uint mul_div255(uint a, uint b) { uint product = a * b + 128u; return (product + (product >> 8u)) >> 8u; }
uint combine_alpha(uint a, uint b) { return mul_div255(a, b); }
float straight_channel(uint premul, uint alpha) { return alpha == 0u ? 0.0 : float(premul) / float(alpha); }
uint scale_premul_u8(uint source, uint factor) {
    if (factor == 0u) return 0u;
    if (factor == CHANNEL_MAX) return source;
    return rgba8_pack(mul_div255(source & CHANNEL_MAX, factor), mul_div255((source >> 8u) & CHANNEL_MAX, factor),
        mul_div255((source >> 16u) & CHANNEL_MAX, factor), mul_div255(source >> 24u, factor));
}
float4 scale_premul_u8_to_unorm(uint source, uint factor) {
    if (factor == 0u || (source >> 24u) == 0u) return float4(0.0,0.0,0.0,0.0);
    float scale = float(factor) * (1.0 / 65025.0);
    return float4(source & CHANNEL_MAX, (source >> 8u) & CHANNEL_MAX, (source >> 16u) & CHANNEL_MAX, source >> 24u) * scale;
}
uint src_over_premul_u8(uint destination, uint source) {
    uint alpha = source >> 24u;
    if (alpha == 0u) return destination;
    if (alpha == CHANNEL_MAX) return source;
    uint inverse = CHANNEL_MAX - alpha;
    return rgba8_pack((source & CHANNEL_MAX) + mul_div255(destination & CHANNEL_MAX, inverse),
        ((source >> 8u) & CHANNEL_MAX) + mul_div255((destination >> 8u) & CHANNEL_MAX, inverse),
        ((source >> 16u) & CHANNEL_MAX) + mul_div255((destination >> 16u) & CHANNEL_MAX, inverse),
        alpha + mul_div255(destination >> 24u, inverse));
}
float4 src_over_premul_unorm(float4 destination, float4 source) {
    if (source.a <= 0.0) return destination;
    if (source.a >= 1.0) return source;
    return source + destination * (1.0 - source.a);
}
uint pack_premul_rgba8(float r, float g, float b, float a) {
    uint4 channels = uint4(clamp(float4(r,g,b,a), 0.0, 1.0) * 255.0 + 0.5);
    return rgba8_pack(channels.x, channels.y, channels.z, channels.w);
}
uint lerp_premul_u8(uint a, uint b, float t) {
    // Stay in the stored channel domain and perform one fused interpolation,
    // matching the production WGSL half-channel rounding contract.
    float4 left = float4(a & CHANNEL_MAX, (a >> 8u) & CHANNEL_MAX, (a >> 16u) & CHANNEL_MAX, a >> 24u);
    float4 right = float4(b & CHANNEL_MAX, (b >> 8u) & CHANNEL_MAX, (b >> 16u) & CHANNEL_MAX, b >> 24u);
    uint4 value = uint4(clamp(mad(right - left, t, left) + 0.5, 0.0, 255.0));
    return rgba8_pack(value.x, value.y, value.z, value.w);
}
float rem_euclid_f32(float value, float modulus) { return value - floor(value / modulus) * modulus; }

#endif // TILEINK_HLSL_SHARED_PIXEL_HLSLI_INCLUDED
