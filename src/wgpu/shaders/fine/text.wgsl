fn src_over_subpixel_mask_u8(dst: u32, src: u32, mask_rgb: u32, clip: u32) -> u32 {
    let sa = src >> 24u;
    var out = dst;
    if (sa != 0u && clip != 0u) {
        let mr = combine_alpha(mask_rgb & 255u, clip);
        let mg = combine_alpha((mask_rgb >> 8u) & 255u, clip);
        let mb = combine_alpha((mask_rgb >> 16u) & 255u, clip);
        if (mr != 0u || mg != 0u || mb != 0u) {
            let cr = mul_div255(sa, mr);
            let cg = mul_div255(sa, mg);
            let cb = mul_div255(sa, mb);
            let ca = max(max(cr, cg), cb);
            let r = mul_div255(src & 255u, mr) + mul_div255(dst & 255u, 255u - cr);
            let g = mul_div255((src >> 8u) & 255u, mg) + mul_div255((dst >> 8u) & 255u, 255u - cg);
            let b = mul_div255((src >> 16u) & 255u, mb) + mul_div255((dst >> 16u) & 255u, 255u - cb);
            let a = ca + mul_div255((dst >> 24u) & 255u, 255u - ca);
            out = rgba8_pack(r, g, b, a);
        }
    }
    return out;
}

fn src_over_mask_linear_u8(dst: u32, src: u32, coverage: u32) -> u32 {
    var out = dst;
    if ((src >> 24u) != 0u && coverage != 0u) {
        let coverage_f = f32(coverage) * (1.0 / 255.0);
        let src_a = f32((src >> 24u) & 255u) * (1.0 / 255.0);
        let dst_a = f32((dst >> 24u) & 255u) * (1.0 / 255.0);
        let src_r = linear_premul_from_srgb8(src & 255u, src_a);
        let src_g = linear_premul_from_srgb8((src >> 8u) & 255u, src_a);
        let src_b = linear_premul_from_srgb8((src >> 16u) & 255u, src_a);
        let dst_r = linear_premul_from_srgb8(dst & 255u, dst_a);
        let dst_g = linear_premul_from_srgb8((dst >> 8u) & 255u, dst_a);
        let dst_b = linear_premul_from_srgb8((dst >> 16u) & 255u, dst_a);
        let out_src_a = src_a * coverage_f;
        let out_a = out_src_a + dst_a * (1.0 - out_src_a);
        out = pack_linear_premul_to_srgb8(
            src_r * coverage_f + dst_r * (1.0 - out_src_a),
            src_g * coverage_f + dst_g * (1.0 - out_src_a),
            src_b * coverage_f + dst_b * (1.0 - out_src_a),
            out_a,
        );
    }
    return out;
}

fn src_over_mask_linear_auto_u8(dst: u32, src: u32, coverage_in: u32) -> u32 {
    let coverage = auto_text_coverage(
        dst,
        src,
        coverage_in,
        TEXT_ALPHA_MASK_CHROMA_SCALE,
        TEXT_ALPHA_MASK_LOW_LUMA_CHROMA_REDUCTION * max(TEXT_ALPHA_MASK_CHROMA_SCALE - TEXT_SUBPIXEL_MASK_CHROMA_SCALE, 0.0),
        TEXT_ALPHA_MASK_LOW_LUMA_CONTRAST_LIMIT,
        TEXT_ALPHA_MASK_APPARENT_AXIS_STRENGTH,
        TEXT_ALPHA_MASK_APPARENT_AXIS_LUMA_LIMIT,
        false,
    );
    return src_over_mask_linear_u8(dst, src, coverage);
}

fn src_over_subpixel_mask_linear_u8(dst: u32, src: u32, mask_rgb: u32, clip: u32) -> u32 {
    var out = dst;
    if ((src >> 24u) != 0u && clip != 0u) {
        let mr = f32(combine_alpha(mask_rgb & 255u, clip)) * (1.0 / 255.0);
        let mg = f32(combine_alpha((mask_rgb >> 8u) & 255u, clip)) * (1.0 / 255.0);
        let mb = f32(combine_alpha((mask_rgb >> 16u) & 255u, clip)) * (1.0 / 255.0);
        if (mr != 0.0 || mg != 0.0 || mb != 0.0) {
            let src_a = f32((src >> 24u) & 255u) * (1.0 / 255.0);
            let dst_a = f32((dst >> 24u) & 255u) * (1.0 / 255.0);
            let src_r = linear_premul_from_srgb8(src & 255u, src_a);
            let src_g = linear_premul_from_srgb8((src >> 8u) & 255u, src_a);
            let src_b = linear_premul_from_srgb8((src >> 16u) & 255u, src_a);
            let dst_r = linear_premul_from_srgb8(dst & 255u, dst_a);
            let dst_g = linear_premul_from_srgb8((dst >> 8u) & 255u, dst_a);
            let dst_b = linear_premul_from_srgb8((dst >> 16u) & 255u, dst_a);
            let cr = src_a * mr;
            let cg = src_a * mg;
            let cb = src_a * mb;
            let ca = max(max(cr, cg), cb);
            let out_a = ca + dst_a * (1.0 - ca);
            out = pack_linear_premul_to_srgb8(
                src_r * mr + dst_r * (1.0 - cr),
                src_g * mg + dst_g * (1.0 - cg),
                src_b * mb + dst_b * (1.0 - cb),
                out_a,
            );
        }
    }
    return out;
}

fn src_over_subpixel_mask_linear_auto_u8(dst: u32, src: u32, mask_rgb: u32, clip: u32) -> u32 {
    let chroma_scale = TEXT_SUBPIXEL_MASK_CHROMA_SCALE;
    let low_luma_chroma_reduction = TEXT_SUBPIXEL_MASK_LOW_LUMA_CHROMA_REDUCTION;
    let low_luma_contrast_limit = TEXT_SUBPIXEL_MASK_LOW_LUMA_CONTRAST_LIMIT;
    let r = auto_text_coverage(
        dst,
        src,
        combine_alpha(mask_rgb & 255u, clip),
        chroma_scale,
        low_luma_chroma_reduction,
        low_luma_contrast_limit,
        0.0,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
        true,
    );
    let g = auto_text_coverage(
        dst,
        src,
        combine_alpha((mask_rgb >> 8u) & 255u, clip),
        chroma_scale,
        low_luma_chroma_reduction,
        low_luma_contrast_limit,
        0.0,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
        true,
    );
    let b = auto_text_coverage(
        dst,
        src,
        combine_alpha((mask_rgb >> 16u) & 255u, clip),
        chroma_scale,
        low_luma_chroma_reduction,
        low_luma_contrast_limit,
        0.0,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
        true,
    );
    let compensated = r | (g << 8u) | (b << 16u);
    let corrected = subpixel_axis_corrected_mask(
        dst,
        src,
        compensated,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_STRENGTH,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
    );
    return src_over_subpixel_mask_linear_u8(dst, src, corrected, 255u);
}

fn auto_text_coverage(
    dst: u32,
    src: u32,
    coverage_in: u32,
    chroma_scale: f32,
    low_luma_chroma_reduction: f32,
    low_luma_contrast_limit: f32,
    apparent_axis_strength: f32,
    apparent_axis_luma_limit: f32,
    destination_chroma_boost: bool,
) -> u32 {
    var out = coverage_in;
    if (coverage_in != 0u && coverage_in != 255u) {
        let src_alpha = f32((src >> 24u) & 255u) * (1.0 / 255.0);
        let dst_alpha = f32((dst >> 24u) & 255u) * (1.0 / 255.0);
        var src_sr = 0.0;
        var src_sg = 0.0;
        var src_sb = 0.0;
        var dst_sr = 0.0;
        var dst_sg = 0.0;
        var dst_sb = 0.0;
        var src_r = 0.0;
        var src_g = 0.0;
        var src_b = 0.0;
        var dst_r = 0.0;
        var dst_g = 0.0;
        var dst_b = 0.0;
        if (src_alpha > 0.0) {
            let inv_alpha = 1.0 / (src_alpha * 255.0);
            src_sr = clamp(f32(src & 255u) * inv_alpha, 0.0, 1.0);
            src_sg = clamp(f32((src >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            src_sb = clamp(f32((src >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
            src_r = srgb_to_linear(src_sr);
            src_g = srgb_to_linear(src_sg);
            src_b = srgb_to_linear(src_sb);
        }
        if (dst_alpha > 0.0) {
            let inv_alpha = 1.0 / (dst_alpha * 255.0);
            dst_sr = clamp(f32(dst & 255u) * inv_alpha, 0.0, 1.0);
            dst_sg = clamp(f32((dst >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            dst_sb = clamp(f32((dst >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
            dst_r = srgb_to_linear(dst_sr);
            dst_g = srgb_to_linear(dst_sg);
            dst_b = srgb_to_linear(dst_sb);
        }

        let src_luma = 0.2126 * src_r + 0.7152 * src_g + 0.0722 * src_b;
        let dst_luma = 0.2126 * dst_r + 0.7152 * dst_g + 0.0722 * dst_b;
        let src_perceptual_luma = 0.2126 * src_sr + 0.7152 * src_sg + 0.0722 * src_sb;
        let dst_perceptual_luma = 0.2126 * dst_sr + 0.7152 * dst_sg + 0.0722 * dst_sb;
        let src_max = max(max(src_r, src_g), src_b);
        let src_min = min(min(src_r, src_g), src_b);
        let dst_max = max(max(dst_r, dst_g), dst_b);
        let dst_min = min(min(dst_r, dst_g), dst_b);
        let src_chroma = clamp(src_max - src_min, 0.0, 1.0);
        let dst_chroma = clamp(dst_max - dst_min, 0.0, 1.0);
        let channel_contrast = max(max(abs(src_r - dst_r), abs(src_g - dst_g)), abs(src_b - dst_b));
        let luma_contrast = abs(src_luma - dst_luma);
        var low_luma_contrast = 0.0;
        if (low_luma_contrast_limit > 0.0) {
            low_luma_contrast = clamp((low_luma_contrast_limit - luma_contrast) / low_luma_contrast_limit, 0.0, 1.0);
        }
        low_luma_contrast = low_luma_contrast * low_luma_contrast;
        var perceptual_light_on_dark_gate = 0.0;
        if (src_perceptual_luma >= dst_perceptual_luma) {
            perceptual_light_on_dark_gate = 1.0;
        }
        let low_luma_chroma_suppression =
            low_luma_chroma_reduction *
            low_luma_contrast *
            perceptual_light_on_dark_gate *
            channel_contrast *
            clamp((src_chroma + dst_chroma) * 0.5, 0.0, 1.0);
        let src_chroma_dominance = clamp((src_chroma - dst_chroma) * 2.0, 0.0, 1.0);
        var source_chroma_contrast_gate = 0.0;
        if (TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT > 0.0) {
            source_chroma_contrast_gate = clamp((TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT - luma_contrast) / TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT, 0.0, 1.0);
        }
        let source_chroma_coverage_boost =
            TEXT_SOURCE_CHROMA_COVERAGE_BOOST *
            source_chroma_contrast_gate *
            src_chroma_dominance *
            src_chroma *
            channel_contrast;
        if (src_luma < dst_luma) {
            let contrast = clamp(dst_luma - src_luma, 0.0, 1.0);
            let hidden_chroma_contrast = max(channel_contrast - contrast, 0.0);
            let dst_chroma_dominance = clamp((dst_chroma - src_chroma) * 2.0, 0.0, 1.0);
            var dark_on_light_chroma = src_chroma;
            if (destination_chroma_boost) {
                dark_on_light_chroma = max(dark_on_light_chroma, dst_chroma * (1.0 - src_max));
            }
            let curve = clamp(
                contrast * (TEXT_DARK_ON_LIGHT_LUMA_BASE - TEXT_DARK_ON_LIGHT_LUMA_TAPER * dst_luma) +
                    TEXT_DARK_ON_LIGHT_CHROMA_BOOST * chroma_scale * hidden_chroma_contrast * dark_on_light_chroma,
                0.0,
                1.0,
            );
            let exponent = max(
                1.0 - TEXT_DARK_ON_LIGHT_COVERAGE_STRENGTH * curve - source_chroma_coverage_boost +
                    low_luma_chroma_suppression * dst_chroma_dominance,
                0.03,
            );
            let compensated = pow(f32(coverage_in) * (1.0 / 255.0), exponent);
            let axis_corrected = apparent_axis_corrected_coverage(
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
                apparent_axis_luma_limit,
            );
            let axis_coverage = f32(axis_corrected) * (1.0 / 255.0);
            if (destination_chroma_boost) {
                // LCD masks benefit from a stronger stem core after filtering. Alpha masks keep
                // their established coverage curve because they have no color fringe to trade.
                let core_contrast = axis_coverage * (1.0 - axis_coverage) *
                    (2.0 * axis_coverage - 1.0);
                out = u32(clamp(
                    axis_coverage + TEXT_DARK_ON_LIGHT_CORE_CONTRAST * core_contrast,
                    0.0,
                    1.0,
                ) * 255.0 + 0.5);
            } else {
                out = axis_corrected;
            }
        } else {
            let contrast = clamp(src_luma - dst_luma, 0.0, 1.0);
            let black_surface = clamp((TEXT_LIGHT_ON_DARK_BLACK_LUMA_LIMIT - dst_luma) / TEXT_LIGHT_ON_DARK_BLACK_LUMA_LIMIT, 0.0, 1.0);
            var high_luma_chroma = 0.0;
            if (src_max > 0.0) {
                high_luma_chroma = src_chroma * max(src_luma / src_max - TEXT_LIGHT_ON_DARK_HIGH_LUMA_THRESHOLD, 0.0);
            }
            let colored_dark_surface =
                clamp((TEXT_LIGHT_ON_COLORED_DARK_LUMA_LIMIT - dst_luma) / TEXT_LIGHT_ON_COLORED_DARK_LUMA_LIMIT, 0.0, 1.0) *
                clamp(dst_chroma * 4.0, 0.0, 1.0);
            let alpha_mask_chroma_excess = max(chroma_scale - TEXT_SUBPIXEL_MASK_CHROMA_SCALE, 0.0);
            let exponent = max(
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
                0.03,
            );
            let compensated = pow(f32(coverage_in) * (1.0 / 255.0), exponent);
            out = apparent_axis_corrected_coverage(
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
                apparent_axis_luma_limit,
            );
        }
    }
    return out;
}

fn apparent_axis_corrected_coverage(
    coverage: f32,
    src_r: f32,
    src_g: f32,
    src_b: f32,
    dst_r: f32,
    dst_g: f32,
    dst_b: f32,
    src_sr: f32,
    src_sg: f32,
    src_sb: f32,
    dst_sr: f32,
    dst_sg: f32,
    dst_sb: f32,
    src_chroma: f32,
    dst_chroma: f32,
    perceptual_luma_contrast: f32,
    strength: f32,
    luma_limit: f32,
) -> u32 {
    var out = u32(clamp(coverage, 0.0, 1.0) * 255.0 + 0.5);
    if (strength > 0.0 && luma_limit > 0.0) {
        let luma_gate = clamp((luma_limit - perceptual_luma_contrast) / luma_limit, 0.0, 1.0);
        let chroma_gate = clamp(max(src_chroma, dst_chroma), 0.0, 1.0);
        if (luma_gate > 0.0 && chroma_gate > 0.0) {
            let axis_r = src_sr - dst_sr;
            let axis_g = src_sg - dst_sg;
            let axis_b = src_sb - dst_sb;
            let denom = axis_r * axis_r + axis_g * axis_g + axis_b * axis_b;
            if (denom > 0.000001) {
                let clamped = clamp(coverage, 0.0, 1.0);
                let mixed_lr = dst_r + (src_r - dst_r) * clamped;
                let mixed_lg = dst_g + (src_g - dst_g) * clamped;
                let mixed_lb = dst_b + (src_b - dst_b) * clamped;
                let mixed_r = linear_to_srgb(mixed_lr);
                let mixed_g = linear_to_srgb(mixed_lg);
                let mixed_b = linear_to_srgb(mixed_lb);
                let projection = clamp(((mixed_r - dst_sr) * axis_r + (mixed_g - dst_sg) * axis_g + (mixed_b - dst_sb) * axis_b) / denom, 0.0, 1.0);
                let derivative = max(
                    (axis_r * linear_to_srgb_derivative(mixed_lr) * (src_r - dst_r) +
                        axis_g * linear_to_srgb_derivative(mixed_lg) * (src_g - dst_g) +
                        axis_b * linear_to_srgb_derivative(mixed_lb) * (src_b - dst_b)) / denom,
                    0.0,
                );
                var correction = 0.0;
                if (derivative > 0.0001) {
                    correction = clamp(strength * luma_gate * chroma_gate, 0.0, 1.0) * (clamped - projection) / clamp(derivative, 0.2, 5.0);
                }
                if (!(correction < 0.0 && perceptual_luma_contrast < luma_limit * 0.05)) {
                    out = u32(clamp(clamped + correction, 0.0, 1.0) * 255.0 + 0.5);
                }
            }
        }
    }
    return out;
}

fn subpixel_axis_corrected_mask(dst: u32, src: u32, mask_rgb: u32, strength: f32, luma_limit: f32) -> u32 {
    var out = mask_rgb;
    if (strength > 0.0 && luma_limit > 0.0) {
        let src_alpha = f32((src >> 24u) & 255u) * (1.0 / 255.0);
        let dst_alpha = f32((dst >> 24u) & 255u) * (1.0 / 255.0);
        var src_sr = 0.0;
        var src_sg = 0.0;
        var src_sb = 0.0;
        var dst_sr = 0.0;
        var dst_sg = 0.0;
        var dst_sb = 0.0;
        if (src_alpha > 0.0) {
            let inv_alpha = 1.0 / (src_alpha * 255.0);
            src_sr = clamp(f32(src & 255u) * inv_alpha, 0.0, 1.0);
            src_sg = clamp(f32((src >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            src_sb = clamp(f32((src >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
        }
        if (dst_alpha > 0.0) {
            let inv_alpha = 1.0 / (dst_alpha * 255.0);
            dst_sr = clamp(f32(dst & 255u) * inv_alpha, 0.0, 1.0);
            dst_sg = clamp(f32((dst >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            dst_sb = clamp(f32((dst >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
        }
        let src_r = srgb_to_linear(src_sr);
        let src_g = srgb_to_linear(src_sg);
        let src_b = srgb_to_linear(src_sb);
        let dst_r = srgb_to_linear(dst_sr);
        let dst_g = srgb_to_linear(dst_sg);
        let dst_b = srgb_to_linear(dst_sb);
        let src_max = max(max(src_r, src_g), src_b);
        let src_min = min(min(src_r, src_g), src_b);
        let dst_max = max(max(dst_r, dst_g), dst_b);
        let dst_min = min(min(dst_r, dst_g), dst_b);
        let chroma_gate = clamp(max(src_max - src_min, dst_max - dst_min), 0.0, 1.0);
        let src_luma = 0.2126 * src_sr + 0.7152 * src_sg + 0.0722 * src_sb;
        let dst_luma = 0.2126 * dst_sr + 0.7152 * dst_sg + 0.0722 * dst_sb;
        let luma_gate = clamp((luma_limit - abs(src_luma - dst_luma)) / luma_limit, 0.0, 1.0);
        if (luma_gate > 0.0 && chroma_gate > 0.0) {
            let axis_r = src_sr - dst_sr;
            let axis_g = src_sg - dst_sg;
            let axis_b = src_sb - dst_sb;
            let denom = axis_r * axis_r + axis_g * axis_g + axis_b * axis_b;
            if (denom > 0.000001) {
                let rendered = src_over_subpixel_mask_linear_u8(dst, src, mask_rgb, 255u);
                let rendered_alpha = f32((rendered >> 24u) & 255u) * (1.0 / 255.0);
                var px_r = 0.0;
                var px_g = 0.0;
                var px_b = 0.0;
                if (rendered_alpha > 0.0) {
                    let inv_alpha = 1.0 / (rendered_alpha * 255.0);
                    px_r = clamp(f32(rendered & 255u) * inv_alpha, 0.0, 1.0);
                    px_g = clamp(f32((rendered >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
                    px_b = clamp(f32((rendered >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
                }
                let projected = clamp(((px_r - dst_sr) * axis_r + (px_g - dst_sg) * axis_g + (px_b - dst_sb) * axis_b) / denom, 0.0, 1.0);
                let target_coverage =
                    f32((mask_rgb & 255u) + ((mask_rgb >> 8u) & 255u) + ((mask_rgb >> 16u) & 255u)) *
                    (1.0 / 765.0);
                let correction = strength * luma_gate * chroma_gate * max(target_coverage - projected, 0.0);
                let r = u32(clamp(f32(mask_rgb & 255u) * (1.0 / 255.0) + correction, 0.0, 1.0) * 255.0 + 0.5);
                let g = u32(clamp(f32((mask_rgb >> 8u) & 255u) * (1.0 / 255.0) + correction, 0.0, 1.0) * 255.0 + 0.5);
                let b = u32(clamp(f32((mask_rgb >> 16u) & 255u) * (1.0 / 255.0) + correction, 0.0, 1.0) * 255.0 + 0.5);
                out = r | (g << 8u) | (b << 16u);
            }
        }
    }
    return out;
}

fn linear_premul_from_srgb8(value: u32, alpha: f32) -> f32 {
    var out = 0.0;
    if (alpha > 0.0) {
        out = srgb_to_linear((f32(value) * (1.0 / 255.0)) / alpha) * alpha;
    }
    return out;
}

fn pack_linear_premul_to_srgb8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    var out = 0u;
    if (a > 0.0) {
        let alpha = clamp(a, 0.0, 1.0);
        let pr = linear_premul_channel_to_srgb8(r, alpha);
        let pg = linear_premul_channel_to_srgb8(g, alpha);
        let pb = linear_premul_channel_to_srgb8(b, alpha);
        let pa = u32(alpha * 255.0 + 0.5);
        out = pr | (pg << 8u) | (pb << 16u) | (pa << 24u);
    }
    return out;
}

fn linear_premul_channel_to_srgb8(value: f32, alpha: f32) -> u32 {
    return u32(linear_to_srgb(clamp(value / alpha, 0.0, 1.0)) * alpha * 255.0 + 0.5);
}

fn srgb_to_linear(value: f32) -> f32 {
    let v = clamp(value, 0.0, 1.0);
    var out = v / 12.92;
    if (v > 0.04045) {
        out = pow((v + 0.055) / 1.055, 2.4);
    }
    return out;
}

fn linear_to_srgb(value: f32) -> f32 {
    let v = clamp(value, 0.0, 1.0);
    var out = v * 12.92;
    if (v > 0.0031308) {
        out = 1.055 * pow(v, 1.0 / 2.4) - 0.055;
    }
    return out;
}

fn linear_to_srgb_derivative(value: f32) -> f32 {
    let v = clamp(value, 0.0, 1.0);
    var out = 12.92;
    if (v > 0.0031308) {
        out = (1.055 / 2.4) * pow(v, 1.0 / 2.4 - 1.0);
    }
    return out;
}
