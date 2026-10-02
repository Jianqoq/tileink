#ifndef TILEINK_HLSL_FINE_TEXT_BASIC_INCLUDED
#define TILEINK_HLSL_FINE_TEXT_BASIC_INCLUDED

#include "../../shared/pixel.hlsli"
#include "gamma.hlsli"

uint src_over_subpixel_mask_u8(uint dst, uint src, uint mask_rgb, uint clip) {
    uint sa = src >> 24u;
    uint result = dst;
    if (sa != 0u && clip != 0u) {
        uint mr = combine_alpha(mask_rgb & 255u, clip);
        uint mg = combine_alpha((mask_rgb >> 8u) & 255u, clip);
        uint mb = combine_alpha((mask_rgb >> 16u) & 255u, clip);
        if (mr != 0u || mg != 0u || mb != 0u) {
            uint cr = mul_div255(sa, mr);
            uint cg = mul_div255(sa, mg);
            uint cb = mul_div255(sa, mb);
            uint ca = max(max(cr, cg), cb);
            uint r = mul_div255(src & 255u, mr) + mul_div255(dst & 255u, 255u - cr);
            uint g = mul_div255((src >> 8u) & 255u, mg) + mul_div255((dst >> 8u) & 255u, 255u - cg);
            uint b = mul_div255((src >> 16u) & 255u, mb) + mul_div255((dst >> 16u) & 255u, 255u - cb);
            uint a = ca + mul_div255((dst >> 24u) & 255u, 255u - ca);
            result = rgba8_pack(r, g, b, a);
        }
    }
    return result;
}

uint src_over_mask_linear_u8(uint dst, uint src, uint coverage) {
    uint result = dst;
    if ((src >> 24u) != 0u && coverage != 0u) {
        float coverage_f = float(coverage) * (1.0 / 255.0);
        float src_a = float((src >> 24u) & 255u) * (1.0 / 255.0);
        float dst_a = float((dst >> 24u) & 255u) * (1.0 / 255.0);
        float src_r = linear_premul_from_srgb8(src & 255u, src_a);
        float src_g = linear_premul_from_srgb8((src >> 8u) & 255u, src_a);
        float src_b = linear_premul_from_srgb8((src >> 16u) & 255u, src_a);
        float dst_r = linear_premul_from_srgb8(dst & 255u, dst_a);
        float dst_g = linear_premul_from_srgb8((dst >> 8u) & 255u, dst_a);
        float dst_b = linear_premul_from_srgb8((dst >> 16u) & 255u, dst_a);
        float out_src_a = src_a * coverage_f;
        float out_a = out_src_a + dst_a * (1.0 - out_src_a);
        result = pack_linear_premul_to_srgb8(
            src_r * coverage_f + dst_r * (1.0 - out_src_a),
            src_g * coverage_f + dst_g * (1.0 - out_src_a),
            src_b * coverage_f + dst_b * (1.0 - out_src_a),
            out_a);
    }
    return result;
}

uint src_over_subpixel_mask_linear_u8(uint dst, uint src, uint mask_rgb, uint clip) {
    uint result = dst;
    if ((src >> 24u) != 0u && clip != 0u) {
        float mr = float(combine_alpha(mask_rgb & 255u, clip)) * (1.0 / 255.0);
        float mg = float(combine_alpha((mask_rgb >> 8u) & 255u, clip)) * (1.0 / 255.0);
        float mb = float(combine_alpha((mask_rgb >> 16u) & 255u, clip)) * (1.0 / 255.0);
        if (mr != 0.0 || mg != 0.0 || mb != 0.0) {
            float src_a = float((src >> 24u) & 255u) * (1.0 / 255.0);
            float dst_a = float((dst >> 24u) & 255u) * (1.0 / 255.0);
            float src_r = linear_premul_from_srgb8(src & 255u, src_a);
            float src_g = linear_premul_from_srgb8((src >> 8u) & 255u, src_a);
            float src_b = linear_premul_from_srgb8((src >> 16u) & 255u, src_a);
            float dst_r = linear_premul_from_srgb8(dst & 255u, dst_a);
            float dst_g = linear_premul_from_srgb8((dst >> 8u) & 255u, dst_a);
            float dst_b = linear_premul_from_srgb8((dst >> 16u) & 255u, dst_a);
            float cr = src_a * mr;
            float cg = src_a * mg;
            float cb = src_a * mb;
            float ca = max(max(cr, cg), cb);
            float out_a = ca + dst_a * (1.0 - ca);
            result = pack_linear_premul_to_srgb8(
                src_r * mr + dst_r * (1.0 - cr),
                src_g * mg + dst_g * (1.0 - cg),
                src_b * mb + dst_b * (1.0 - cb),
                out_a);
        }
    }
    return result;
}

#endif
