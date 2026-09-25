#ifndef TILEINK_HLSL_FINE_TEXT_AXIS_INCLUDED
#define TILEINK_HLSL_FINE_TEXT_AXIS_INCLUDED

#include "gamma.hlsli"
#include "basic.hlsli"

uint apparent_axis_corrected_coverage(float coverage, float src_r, float src_g, float src_b, float dst_r, float dst_g, float dst_b, float src_sr, float src_sg, float src_sb, float dst_sr, float dst_sg, float dst_sb, float src_chroma, float dst_chroma, float perceptual_luma_contrast, float strength, float luma_limit) {
    uint result = uint(clamp(coverage, 0.0, 1.0) * 255.0 + 0.5);
    if (strength > 0.0 && luma_limit > 0.0) {
        float luma_gate = clamp((luma_limit - perceptual_luma_contrast) / luma_limit, 0.0, 1.0);
        float chroma_gate = clamp(max(src_chroma, dst_chroma), 0.0, 1.0);
        if (luma_gate > 0.0 && chroma_gate > 0.0) {
            float axis_r = src_sr - dst_sr;
            float axis_g = src_sg - dst_sg;
            float axis_b = src_sb - dst_sb;
            float denom = axis_r * axis_r + axis_g * axis_g + axis_b * axis_b;
            if (denom > 0.000001) {
                float clamped = clamp(coverage, 0.0, 1.0);
                float mixed_lr = dst_r + (src_r - dst_r) * clamped;
                float mixed_lg = dst_g + (src_g - dst_g) * clamped;
                float mixed_lb = dst_b + (src_b - dst_b) * clamped;
                float mixed_r = linear_to_srgb(mixed_lr);
                float mixed_g = linear_to_srgb(mixed_lg);
                float mixed_b = linear_to_srgb(mixed_lb);
                float projection = clamp(((mixed_r - dst_sr) * axis_r + (mixed_g - dst_sg) * axis_g + (mixed_b - dst_sb) * axis_b) / denom, 0.0, 1.0);
                float derivative = max(
                    (axis_r * linear_to_srgb_derivative(mixed_lr) * (src_r - dst_r) +
                        axis_g * linear_to_srgb_derivative(mixed_lg) * (src_g - dst_g) +
                        axis_b * linear_to_srgb_derivative(mixed_lb) * (src_b - dst_b)) / denom,
                    0.0);
                float correction = 0.0;
                if (derivative > 0.0001) {
                    correction = clamp(strength * luma_gate * chroma_gate, 0.0, 1.0) * (clamped - projection) / clamp(derivative, 0.2, 5.0);
                }
                if (!(correction < 0.0 && perceptual_luma_contrast < luma_limit * 0.05)) {
                    result = uint(clamp(clamped + correction, 0.0, 1.0) * 255.0 + 0.5);
                }
            }
        }
    }
    return result;
}

uint subpixel_axis_corrected_mask(uint dst, uint src, uint mask_rgb, float strength, float luma_limit) {
    uint result = mask_rgb;
    if (strength > 0.0 && luma_limit > 0.0) {
        float src_alpha = float((src >> 24u) & 255u) * (1.0 / 255.0);
        float dst_alpha = float((dst >> 24u) & 255u) * (1.0 / 255.0);
        float src_sr = 0.0;
        float src_sg = 0.0;
        float src_sb = 0.0;
        float dst_sr = 0.0;
        float dst_sg = 0.0;
        float dst_sb = 0.0;
        if (src_alpha > 0.0) {
            float inv_alpha = 1.0 / (src_alpha * 255.0);
            src_sr = clamp(float(src & 255u) * inv_alpha, 0.0, 1.0);
            src_sg = clamp(float((src >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            src_sb = clamp(float((src >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
        }
        if (dst_alpha > 0.0) {
            float inv_alpha = 1.0 / (dst_alpha * 255.0);
            dst_sr = clamp(float(dst & 255u) * inv_alpha, 0.0, 1.0);
            dst_sg = clamp(float((dst >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            dst_sb = clamp(float((dst >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
        }
        float src_r = srgb_to_linear(src_sr);
        float src_g = srgb_to_linear(src_sg);
        float src_b = srgb_to_linear(src_sb);
        float dst_r = srgb_to_linear(dst_sr);
        float dst_g = srgb_to_linear(dst_sg);
        float dst_b = srgb_to_linear(dst_sb);
        float src_max = max(max(src_r, src_g), src_b);
        float src_min = min(min(src_r, src_g), src_b);
        float dst_max = max(max(dst_r, dst_g), dst_b);
        float dst_min = min(min(dst_r, dst_g), dst_b);
        float chroma_gate = clamp(max(src_max - src_min, dst_max - dst_min), 0.0, 1.0);
        float src_luma = 0.2126 * src_sr + 0.7152 * src_sg + 0.0722 * src_sb;
        float dst_luma = 0.2126 * dst_sr + 0.7152 * dst_sg + 0.0722 * dst_sb;
        float luma_gate = clamp((luma_limit - abs(src_luma - dst_luma)) / luma_limit, 0.0, 1.0);
        if (luma_gate > 0.0 && chroma_gate > 0.0) {
            float axis_r = src_sr - dst_sr;
            float axis_g = src_sg - dst_sg;
            float axis_b = src_sb - dst_sb;
            float denom = axis_r * axis_r + axis_g * axis_g + axis_b * axis_b;
            if (denom > 0.000001) {
                uint rendered = src_over_subpixel_mask_linear_u8(dst, src, mask_rgb, 255u);
                float rendered_alpha = float((rendered >> 24u) & 255u) * (1.0 / 255.0);
                float px_r = 0.0;
                float px_g = 0.0;
                float px_b = 0.0;
                if (rendered_alpha > 0.0) {
                    float inv_alpha = 1.0 / (rendered_alpha * 255.0);
                    px_r = clamp(float(rendered & 255u) * inv_alpha, 0.0, 1.0);
                    px_g = clamp(float((rendered >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
                    px_b = clamp(float((rendered >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
                }
                float projected = clamp(((px_r - dst_sr) * axis_r + (px_g - dst_sg) * axis_g + (px_b - dst_sb) * axis_b) / denom, 0.0, 1.0);
                float target_coverage =
                    float((mask_rgb & 255u) + ((mask_rgb >> 8u) & 255u) + ((mask_rgb >> 16u) & 255u)) *
                    (1.0 / 765.0);
                float correction = strength * luma_gate * chroma_gate * max(target_coverage - projected, 0.0);
                uint r = uint(clamp(float(mask_rgb & 255u) * (1.0 / 255.0) + correction, 0.0, 1.0) * 255.0 + 0.5);
                uint g = uint(clamp(float((mask_rgb >> 8u) & 255u) * (1.0 / 255.0) + correction, 0.0, 1.0) * 255.0 + 0.5);
                uint b = uint(clamp(float((mask_rgb >> 16u) & 255u) * (1.0 / 255.0) + correction, 0.0, 1.0) * 255.0 + 0.5);
                result = r | (g << 8u) | (b << 16u);
            }
        }
    }
    return result;
}

#endif
