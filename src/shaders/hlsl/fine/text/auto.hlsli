#ifndef TILEINK_HLSL_FINE_TEXT_AUTO_INCLUDED
#define TILEINK_HLSL_FINE_TEXT_AUTO_INCLUDED

#include "basic.hlsli"

float3 text_foreground(uint source) {
    float alpha = float(source >> 24u);
    return alpha > 0.0 ? clamp(float3(source & 255u, (source >> 8u) & 255u, (source >> 16u) & 255u) / alpha, 0.0, 1.0) : float3(0.0, 0.0, 0.0);
}

// Correct coverage once before encoded-color source-over, including offscreen
// groups. Mixing text in linear light only at its first draw changed its weight
// when the RGBA layer was later composited. The complementary-background LUT
// principle keeps that representation consistent without a second rasterizer.
float text_correcting_coverage(uint coverage, float foreground) {
    if (coverage == 0u || coverage == 255u) return float(coverage) * (1.0 / 255.0);
    float raw = float(coverage) * (1.0 / 255.0);
    float background = 1.0 - foreground;
    float bg_linear = srgb_to_linear(background);
    float enhanced = raw + raw * (1.0 - raw) * bg_linear;
    float difference = foreground - background;
    // Near equal colors make the inverse blend ill-conditioned. The continuous
    // coverage limit preserves its contrast; this is the correcting-LUT safeguard.
    if (abs(difference) < (1.0 / 256.0)) return enhanced;
    float output = linear_to_srgb(lerp(bg_linear, srgb_to_linear(foreground), enhanced));
    return clamp((output - background) / difference, 0.0, 1.0);
}

uint src_over_mask_linear_auto_u8(uint destination, uint source, uint coverage) {
    if ((source >> 24u) == 0u || coverage == 0u) return destination;
    float foreground = dot(text_foreground(source), float3(0.2126, 0.7152, 0.0722));
    float corrected = text_correcting_coverage(coverage, foreground);
    return src_over_subpixel_mask_f32(destination, source, float3(corrected, corrected, corrected));
}

uint src_over_subpixel_mask_linear_auto_u8(uint destination, uint source, uint mask, uint clip) {
    if ((source >> 24u) == 0u || clip == 0u) return destination;
    float3 foreground = text_foreground(source);
    // Correct each channel before the transparent-destination scalar reduction.
    // Averaging raw masks before this nonlinear transfer changed fading text ink.
    float3 corrected = float3(
        text_correcting_coverage(combine_alpha(mask & 255u, clip), foreground.r),
        text_correcting_coverage(combine_alpha((mask >> 8u) & 255u, clip), foreground.g),
        text_correcting_coverage(combine_alpha((mask >> 16u) & 255u, clip), foreground.b));
    return src_over_subpixel_mask_f32(destination, source, corrected);
}

#endif
