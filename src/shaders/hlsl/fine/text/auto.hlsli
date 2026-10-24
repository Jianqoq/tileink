#ifndef TILEINK_HLSL_FINE_TEXT_AUTO_INCLUDED
#define TILEINK_HLSL_FINE_TEXT_AUTO_INCLUDED

#include "../../shared/pixel.hlsli"
#include "constants.hlsli"
#include "basic.hlsli"
#include "coverage.hlsli"
#include "axis.hlsli"

uint src_over_mask_linear_auto_u8(uint dst, uint src, uint coverage_in) {
    uint coverage = auto_text_coverage(
        dst,
        src,
        coverage_in,
        TEXT_ALPHA_MASK_CHROMA_SCALE,
        TEXT_ALPHA_MASK_LOW_LUMA_CHROMA_REDUCTION * max(TEXT_ALPHA_MASK_CHROMA_SCALE - TEXT_SUBPIXEL_MASK_CHROMA_SCALE, 0.0),
        TEXT_ALPHA_MASK_LOW_LUMA_CONTRAST_LIMIT,
        TEXT_ALPHA_MASK_APPARENT_AXIS_STRENGTH,
        TEXT_ALPHA_MASK_APPARENT_AXIS_LUMA_LIMIT,
        false);
    return src_over_mask_linear_u8(dst, src, coverage);
}

uint src_over_subpixel_mask_linear_auto_u8(uint dst, uint src, uint mask_rgb, uint clip) {
    float chroma_scale = TEXT_SUBPIXEL_MASK_CHROMA_SCALE;
    float low_luma_chroma_reduction = TEXT_SUBPIXEL_MASK_LOW_LUMA_CHROMA_REDUCTION;
    float low_luma_contrast_limit = TEXT_SUBPIXEL_MASK_LOW_LUMA_CONTRAST_LIMIT;
    uint r = auto_text_coverage(
        dst,
        src,
        combine_alpha(mask_rgb & 255u, clip),
        chroma_scale,
        low_luma_chroma_reduction,
        low_luma_contrast_limit,
        0.0,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
        true);
    uint g = auto_text_coverage(
        dst,
        src,
        combine_alpha((mask_rgb >> 8u) & 255u, clip),
        chroma_scale,
        low_luma_chroma_reduction,
        low_luma_contrast_limit,
        0.0,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
        true);
    uint b = auto_text_coverage(
        dst,
        src,
        combine_alpha((mask_rgb >> 16u) & 255u, clip),
        chroma_scale,
        low_luma_chroma_reduction,
        low_luma_contrast_limit,
        0.0,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
        true);
    uint compensated = r | (g << 8u) | (b << 16u);
    uint corrected = subpixel_axis_corrected_mask(
        dst,
        src,
        compensated,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_STRENGTH,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT);
    return src_over_subpixel_mask_linear_u8(dst, src, corrected, 255u);
}

#endif
