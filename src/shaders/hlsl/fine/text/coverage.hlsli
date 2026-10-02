#ifndef TILEINK_HLSL_FINE_TEXT_COVERAGE_INCLUDED
#define TILEINK_HLSL_FINE_TEXT_COVERAGE_INCLUDED

#include "constants.hlsli"
#include "gamma.hlsli"
#include "axis.hlsli"

uint auto_text_coverage(uint dst, uint src, uint coverage_in, float chroma_scale, float low_luma_chroma_reduction, float low_luma_contrast_limit, float apparent_axis_strength, float apparent_axis_luma_limit, bool destination_chroma_boost) {
    uint result = coverage_in;
    if (coverage_in != 0u && coverage_in != 255u) {
        float src_alpha = float((src >> 24u) & 255u) * (1.0 / 255.0);
        float dst_alpha = float((dst >> 24u) & 255u) * (1.0 / 255.0);
        float src_sr = 0.0;
        float src_sg = 0.0;
        float src_sb = 0.0;
        float dst_sr = 0.0;
        float dst_sg = 0.0;
        float dst_sb = 0.0;
        float src_r = 0.0;
        float src_g = 0.0;
        float src_b = 0.0;
        float dst_r = 0.0;
        float dst_g = 0.0;
        float dst_b = 0.0;
        if (src_alpha > 0.0) {
            float inv_alpha = 1.0 / (src_alpha * 255.0);
            src_sr = clamp(float(src & 255u) * inv_alpha, 0.0, 1.0);
            src_sg = clamp(float((src >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            src_sb = clamp(float((src >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
            src_r = srgb_to_linear(src_sr);
            src_g = srgb_to_linear(src_sg);
            src_b = srgb_to_linear(src_sb);
        }
        if (dst_alpha > 0.0) {
            float inv_alpha = 1.0 / (dst_alpha * 255.0);
            dst_sr = clamp(float(dst & 255u) * inv_alpha, 0.0, 1.0);
            dst_sg = clamp(float((dst >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            dst_sb = clamp(float((dst >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
            dst_r = srgb_to_linear(dst_sr);
            dst_g = srgb_to_linear(dst_sg);
            dst_b = srgb_to_linear(dst_sb);
        }

        float src_luma = 0.2126 * src_r + 0.7152 * src_g + 0.0722 * src_b;
        float dst_luma = 0.2126 * dst_r + 0.7152 * dst_g + 0.0722 * dst_b;
        float src_perceptual_luma = 0.2126 * src_sr + 0.7152 * src_sg + 0.0722 * src_sb;
        float dst_perceptual_luma = 0.2126 * dst_sr + 0.7152 * dst_sg + 0.0722 * dst_sb;
        float src_max = max(max(src_r, src_g), src_b);
        float src_min = min(min(src_r, src_g), src_b);
        float dst_max = max(max(dst_r, dst_g), dst_b);
        float dst_min = min(min(dst_r, dst_g), dst_b);
        float src_chroma = clamp(src_max - src_min, 0.0, 1.0);
        float dst_chroma = clamp(dst_max - dst_min, 0.0, 1.0);
        float channel_contrast = max(max(abs(src_r - dst_r), abs(src_g - dst_g)), abs(src_b - dst_b));
        float luma_contrast = abs(src_luma - dst_luma);
        float low_luma_contrast = 0.0;
        if (low_luma_contrast_limit > 0.0) {
            low_luma_contrast = clamp((low_luma_contrast_limit - luma_contrast) / low_luma_contrast_limit, 0.0, 1.0);
        }
        low_luma_contrast = low_luma_contrast * low_luma_contrast;
        float perceptual_light_on_dark_gate = 0.0;
        if (src_perceptual_luma >= dst_perceptual_luma) {
            perceptual_light_on_dark_gate = 1.0;
        }
        float low_luma_chroma_suppression =
            low_luma_chroma_reduction *
            low_luma_contrast *
            perceptual_light_on_dark_gate *
            channel_contrast *
            clamp((src_chroma + dst_chroma) * 0.5, 0.0, 1.0);
        float src_chroma_dominance = clamp((src_chroma - dst_chroma) * 2.0, 0.0, 1.0);
        float source_chroma_contrast_gate = 0.0;
        if (TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT > 0.0) {
            source_chroma_contrast_gate = clamp((TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT - luma_contrast) / TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT, 0.0, 1.0);
        }
        float source_chroma_coverage_boost =
            TEXT_SOURCE_CHROMA_COVERAGE_BOOST *
            source_chroma_contrast_gate *
            src_chroma_dominance *
            src_chroma *
            channel_contrast;
        if (src_luma < dst_luma) {
            float contrast = clamp(dst_luma - src_luma, 0.0, 1.0);
            float hidden_chroma_contrast = max(channel_contrast - contrast, 0.0);
            float dst_chroma_dominance = clamp((dst_chroma - src_chroma) * 2.0, 0.0, 1.0);
            float dark_on_light_chroma = src_chroma;
            if (destination_chroma_boost) {
                dark_on_light_chroma = max(dark_on_light_chroma, dst_chroma * (1.0 - src_max));
            }
            float curve = clamp(
                contrast * (TEXT_DARK_ON_LIGHT_LUMA_BASE - TEXT_DARK_ON_LIGHT_LUMA_TAPER * dst_luma) +
                    TEXT_DARK_ON_LIGHT_CHROMA_BOOST * chroma_scale * hidden_chroma_contrast * dark_on_light_chroma,
                0.0,
                1.0);
            float exponent = max(
                1.0 - TEXT_DARK_ON_LIGHT_COVERAGE_STRENGTH * curve - source_chroma_coverage_boost +
                    low_luma_chroma_suppression * dst_chroma_dominance,
                0.03);
            float compensated = pow(float(coverage_in) * (1.0 / 255.0), exponent);
            uint axis_corrected = apparent_axis_corrected_coverage(
                compensated,
                src_r,
                src_g,
                src_b,
                dst_r,
                dst_g,
                dst_b,
                src_sr,
                src_sg,
                src_sb,
                dst_sr,
                dst_sg,
                dst_sb,
                src_chroma,
                dst_chroma,
                abs(src_perceptual_luma - dst_perceptual_luma),
                apparent_axis_strength,
                apparent_axis_luma_limit);
            float axis_coverage = float(axis_corrected) * (1.0 / 255.0);
            if (destination_chroma_boost) {
                // LCD masks benefit from a stronger stem core after filtering. Alpha masks keep
                // their established coverage curve because they have no color fringe to trade.
                float core_contrast = axis_coverage * (1.0 - axis_coverage) *
                    (2.0 * axis_coverage - 1.0);
                result = uint(clamp(
                    axis_coverage + TEXT_DARK_ON_LIGHT_CORE_CONTRAST * core_contrast,
                    0.0,
                    1.0) * 255.0 + 0.5);
            } else {
                result = axis_corrected;
            }
        } else {
            float contrast = clamp(src_luma - dst_luma, 0.0, 1.0);
            float black_surface = clamp((TEXT_LIGHT_ON_DARK_BLACK_LUMA_LIMIT - dst_luma) / TEXT_LIGHT_ON_DARK_BLACK_LUMA_LIMIT, 0.0, 1.0);
            float high_luma_chroma = 0.0;
            if (src_max > 0.0) {
                high_luma_chroma = src_chroma * max(src_luma / src_max - TEXT_LIGHT_ON_DARK_HIGH_LUMA_THRESHOLD, 0.0);
            }
            float colored_dark_surface =
                clamp((TEXT_LIGHT_ON_COLORED_DARK_LUMA_LIMIT - dst_luma) / TEXT_LIGHT_ON_COLORED_DARK_LUMA_LIMIT, 0.0, 1.0) *
                clamp(dst_chroma * 4.0, 0.0, 1.0);
            float alpha_mask_chroma_excess = max(chroma_scale - TEXT_SUBPIXEL_MASK_CHROMA_SCALE, 0.0);
            float exponent = max(
                1.0 +
                    black_surface *
                        (TEXT_LIGHT_ON_DARK_COVERAGE_REDUCTION * contrast * src_luma +
                            TEXT_LIGHT_ON_DARK_CHROMA_REDUCTION * chroma_scale * src_chroma * src_max +
                            TEXT_LIGHT_ON_DARK_HIGH_LUMA_CHROMA_REDUCTION * chroma_scale * high_luma_chroma) +
                    TEXT_LIGHT_ON_COLORED_DARK_CHROMA_REDUCTION *
                        alpha_mask_chroma_excess *
                        colored_dark_surface *
                        src_chroma *
                        channel_contrast +
                    low_luma_chroma_suppression -
                    source_chroma_coverage_boost * (1.0 - black_surface),
                0.03);
            float compensated = pow(float(coverage_in) * (1.0 / 255.0), exponent);
            result = apparent_axis_corrected_coverage(
                compensated,
                src_r,
                src_g,
                src_b,
                dst_r,
                dst_g,
                dst_b,
                src_sr,
                src_sg,
                src_sb,
                dst_sr,
                dst_sg,
                dst_sb,
                src_chroma,
                dst_chroma,
                abs(src_perceptual_luma - dst_perceptual_luma),
                apparent_axis_strength,
                apparent_axis_luma_limit);
        }
    }
    return result;
}

#endif
