fn blend_premul_u8(dst: u32, src: u32, mode: u32) -> u32 {
    let mix = mode & 255u;
    let compose = (mode >> 8u) & 255u;
    let inv = 1.0 / 255.0;
    let sr = f32(src & 255u) * inv;
    let sg = f32((src >> 8u) & 255u) * inv;
    let sb = f32((src >> 16u) & 255u) * inv;
    let sa = f32((src >> 24u) & 255u) * inv;
    let dr = f32(dst & 255u) * inv;
    let dg = f32((dst >> 8u) & 255u) * inv;
    let db = f32((dst >> 16u) & 255u) * inv;
    let da = f32((dst >> 24u) & 255u) * inv;

    var out_r = sr + dr * (1.0 - sa);
    var out_g = sg + dg * (1.0 - sa);
    var out_b = sb + db * (1.0 - sa);
    var out_a = sa + da * (1.0 - sa);

    if (mix == 0u && compose == 3u) {
    } else if (mix == 0u && compose == 2u) {
        out_r = dr;
        out_g = dg;
        out_b = db;
        out_a = da;
    } else if (mix == 0u && compose == 0u) {
        out_r = 0.0;
        out_g = 0.0;
        out_b = 0.0;
        out_a = 0.0;
    } else if (mix == 0u && compose == 1u) {
        out_r = sr;
        out_g = sg;
        out_b = sb;
        out_a = sa;
    } else if (mix == 0u) {
        let src_factor = compose_src_factor(compose, sa, da);
        let dst_factor = compose_dst_factor(compose, sa, da);
        out_r = sr * src_factor + dr * dst_factor;
        out_g = sg * src_factor + dg * dst_factor;
        out_b = sb * src_factor + db * dst_factor;
        out_a = sa * src_factor + da * dst_factor;
        if (compose == 13u) {
            out_r = min(out_r, 1.0);
            out_g = min(out_g, 1.0);
            out_b = min(out_b, 1.0);
            out_a = min(out_a, 1.0);
        }
    } else if (compose == 3u && mix == 6u) {
        out_r = color_dodge_premul(sr, dr, sa, da);
        out_g = color_dodge_premul(sg, dg, sa, da);
        out_b = color_dodge_premul(sb, db, sa, da);
        out_a = sa + da * (1.0 - sa);
    } else if (compose == 3u && mix == 7u) {
        out_r = color_burn_premul(sr, dr, sa, da);
        out_g = color_burn_premul(sg, dg, sa, da);
        out_b = color_burn_premul(sb, db, sa, da);
        out_a = sa + da * (1.0 - sa);
    } else {
        let src_alpha = clamp(sa, 0.0, 1.0);
        let dst_alpha = clamp(da, 0.0, 1.0);
        let src_r = unpremul_channel(sr, src_alpha);
        let src_g = unpremul_channel(sg, src_alpha);
        let src_b = unpremul_channel(sb, src_alpha);
        let dst_r = unpremul_channel(dr, dst_alpha);
        let dst_g = unpremul_channel(dg, dst_alpha);
        let dst_b = unpremul_channel(db, dst_alpha);
        let mixed_r = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 0u);
        let mixed_g = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 1u);
        let mixed_b = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 2u);
        let effective_r = src_alpha * ((1.0 - dst_alpha) * src_r + dst_alpha * mixed_r);
        let effective_g = src_alpha * ((1.0 - dst_alpha) * src_g + dst_alpha * mixed_g);
        let effective_b = src_alpha * ((1.0 - dst_alpha) * src_b + dst_alpha * mixed_b);
        let src_factor = compose_src_factor(compose, src_alpha, dst_alpha);
        let dst_factor = compose_dst_factor(compose, src_alpha, dst_alpha);
        out_r = effective_r * src_factor + dr * dst_factor;
        out_g = effective_g * src_factor + dg * dst_factor;
        out_b = effective_b * src_factor + db * dst_factor;
        out_a = src_alpha * src_factor + da * dst_factor;
    }

    return pack_premul_rgba8(out_r, out_g, out_b, out_a);
}

fn compose_src_factor(compose: u32, src_alpha: f32, dst_alpha: f32) -> f32 {
    _ = src_alpha;
    var factor = 1.0;
    if (compose == 0u || compose == 2u || compose == 6u || compose == 8u) {
        factor = 0.0;
    } else if (compose == 4u) {
        factor = 1.0 - dst_alpha;
    } else if (compose == 5u || compose == 9u) {
        factor = dst_alpha;
    } else if (compose == 7u || compose == 10u || compose == 11u) {
        factor = 1.0 - dst_alpha;
    }
    return factor;
}

fn compose_dst_factor(compose: u32, src_alpha: f32, dst_alpha: f32) -> f32 {
    _ = dst_alpha;
    var factor = 1.0 - src_alpha;
    if (compose == 0u || compose == 1u || compose == 5u || compose == 7u) {
        factor = 0.0;
    } else if (compose == 2u || compose == 4u) {
        factor = 1.0;
    } else if (compose == 6u || compose == 10u) {
        factor = src_alpha;
    } else if (compose == 8u || compose == 9u || compose == 11u) {
        factor = 1.0 - src_alpha;
    } else if (compose == 12u || compose == 13u) {
        factor = 1.0;
    }
    return factor;
}

fn unpremul_channel(value: f32, alpha: f32) -> f32 {
    var out = 0.0;
    if (alpha > 0.0) {
        out = value / alpha;
    }
    return out;
}

fn mix_rgb_channel(
    dst_r: f32,
    dst_g: f32,
    dst_b: f32,
    src_r: f32,
    src_g: f32,
    src_b: f32,
    mix: u32,
    channel: u32,
) -> f32 {
    var r = src_r;
    var g = src_g;
    var b = src_b;
    if (mix == 1u) {
        r = dst_r * src_r;
        g = dst_g * src_g;
        b = dst_b * src_b;
    } else if (mix == 2u) {
        r = dst_r + src_r - dst_r * src_r;
        g = dst_g + src_g - dst_g * src_g;
        b = dst_b + src_b - dst_b * src_b;
    } else if (mix == 3u) {
        r = overlay(dst_r, src_r);
        g = overlay(dst_g, src_g);
        b = overlay(dst_b, src_b);
    } else if (mix == 4u) {
        r = min(dst_r, src_r);
        g = min(dst_g, src_g);
        b = min(dst_b, src_b);
    } else if (mix == 5u) {
        r = max(dst_r, src_r);
        g = max(dst_g, src_g);
        b = max(dst_b, src_b);
    } else if (mix == 6u) {
        r = color_dodge(dst_r, src_r);
        g = color_dodge(dst_g, src_g);
        b = color_dodge(dst_b, src_b);
    } else if (mix == 7u) {
        r = color_burn(dst_r, src_r);
        g = color_burn(dst_g, src_g);
        b = color_burn(dst_b, src_b);
    } else if (mix == 8u) {
        r = overlay(src_r, dst_r);
        g = overlay(src_g, dst_g);
        b = overlay(src_b, dst_b);
    } else if (mix == 9u) {
        r = soft_light(dst_r, src_r);
        g = soft_light(dst_g, src_g);
        b = soft_light(dst_b, src_b);
    } else if (mix == 10u) {
        r = abs(dst_r - src_r);
        g = abs(dst_g - src_g);
        b = abs(dst_b - src_b);
    } else if (mix == 11u) {
        r = dst_r + src_r - 2.0 * dst_r * src_r;
        g = dst_g + src_g - 2.0 * dst_g * src_g;
        b = dst_b + src_b - 2.0 * dst_b * src_b;
    } else if (mix == 12u) {
        let sat_dst = sat3(dst_r, dst_g, dst_b);
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        let sr = set_sat_channel(src_r, src_g, src_b, sat_dst, 0u);
        let sg = set_sat_channel(src_r, src_g, src_b, sat_dst, 1u);
        let sb = set_sat_channel(src_r, src_g, src_b, sat_dst, 2u);
        r = set_lum_channel(sr, sg, sb, lum_dst, 0u);
        g = set_lum_channel(sr, sg, sb, lum_dst, 1u);
        b = set_lum_channel(sr, sg, sb, lum_dst, 2u);
    } else if (mix == 13u) {
        let sat_src = sat3(src_r, src_g, src_b);
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        let dr = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 0u);
        let dg = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 1u);
        let db = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 2u);
        r = set_lum_channel(dr, dg, db, lum_dst, 0u);
        g = set_lum_channel(dr, dg, db, lum_dst, 1u);
        b = set_lum_channel(dr, dg, db, lum_dst, 2u);
    } else if (mix == 14u) {
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        r = set_lum_channel(src_r, src_g, src_b, lum_dst, 0u);
        g = set_lum_channel(src_r, src_g, src_b, lum_dst, 1u);
        b = set_lum_channel(src_r, src_g, src_b, lum_dst, 2u);
    } else if (mix == 15u) {
        let lum_src = lum3(src_r, src_g, src_b);
        r = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 0u);
        g = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 1u);
        b = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 2u);
    }

    if (channel == 0u) {
        return r;
    } else if (channel == 1u) {
        return g;
    }
    return b;
}

fn overlay(dst: f32, src: f32) -> f32 {
    if (dst <= 0.5) {
        return 2.0 * dst * src;
    }
    return 1.0 - 2.0 * (1.0 - dst) * (1.0 - src);
}

fn color_dodge(dst: f32, src: f32) -> f32 {
    var out = 1.0;
    if (src < 1.0) {
        out = min(dst / (1.0 - src), 1.0);
    }
    return out;
}

fn color_burn(dst: f32, src: f32) -> f32 {
    var out = 0.0;
    if (src > 0.0) {
        out = 1.0 - min((1.0 - dst) / src, 1.0);
    }
    return out;
}

fn color_dodge_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    var out = src * (1.0 - dst_alpha);
    if (dst > 0.0) {
        if (src >= src_alpha) {
            out = src + dst * (1.0 - src_alpha);
        } else {
            out = src_alpha * min(dst_alpha, (dst * src_alpha) / (src_alpha - src)) +
                src * (1.0 - dst_alpha) +
                dst * (1.0 - src_alpha);
        }
    }
    return out;
}

fn color_burn_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    var out = dst + src * (1.0 - dst_alpha);
    if (dst < dst_alpha) {
        if (src <= 0.0) {
            out = dst * (1.0 - src_alpha);
        } else {
            out = src_alpha * (dst_alpha - min(dst_alpha, ((dst_alpha - dst) * src_alpha) / src)) +
                src * (1.0 - dst_alpha) +
                dst * (1.0 - src_alpha);
        }
    }
    return out;
}

fn soft_light(dst: f32, src: f32) -> f32 {
    var out = dst - (1.0 - 2.0 * src) * dst * (1.0 - dst);
    if (src > 0.5) {
        var d = sqrt(dst);
        if (dst <= 0.25) {
            d = ((16.0 * dst - 12.0) * dst + 4.0) * dst;
        }
        out = dst + (2.0 * src - 1.0) * (d - dst);
    }
    return out;
}

fn lum3(r: f32, g: f32, b: f32) -> f32 {
    return 0.3 * r + 0.59 * g + 0.11 * b;
}

fn sat3(r: f32, g: f32, b: f32) -> f32 {
    return max(max(r, g), b) - min(min(r, g), b);
}

fn set_lum_channel(r: f32, g: f32, b: f32, lum: f32, channel: u32) -> f32 {
    let d = lum - lum3(r, g, b);
    return clip_color_channel(r + d, g + d, b + d, channel);
}

fn clip_color_channel(r: f32, g: f32, b: f32, channel: u32) -> f32 {
    let lum = lum3(r, g, b);
    let min_c = min(min(r, g), b);
    let max_c = max(max(r, g), b);
    var out_r = r;
    var out_g = g;
    var out_b = b;
    if (min_c < 0.0) {
        out_r = lum + (out_r - lum) * lum / (lum - min_c);
        out_g = lum + (out_g - lum) * lum / (lum - min_c);
        out_b = lum + (out_b - lum) * lum / (lum - min_c);
    }
    if (max_c > 1.0) {
        out_r = lum + (out_r - lum) * (1.0 - lum) / (max_c - lum);
        out_g = lum + (out_g - lum) * (1.0 - lum) / (max_c - lum);
        out_b = lum + (out_b - lum) * (1.0 - lum) / (max_c - lum);
    }
    if (channel == 0u) {
        return out_r;
    } else if (channel == 1u) {
        return out_g;
    }
    return out_b;
}

fn set_sat_channel(r: f32, g: f32, b: f32, sat: f32, channel: u32) -> f32 {
    var min_ix = 0u;
    if (r <= g && r <= b) {
    } else if (g <= b) {
        min_ix = 1u;
    } else {
        min_ix = 2u;
    }

    var max_ix = 0u;
    if (r >= g && r >= b) {
    } else if (g >= b) {
        max_ix = 1u;
    } else {
        max_ix = 2u;
    }

    var out_r = 0.0;
    var out_g = 0.0;
    var out_b = 0.0;
    if (min_ix != max_ix) {
        let mid_ix = 3u - min_ix - max_ix;
        let min_v = channel_value(r, g, b, min_ix);
        let mid_v = channel_value(r, g, b, mid_ix);
        let max_v = channel_value(r, g, b, max_ix);
        var new_mid = 0.0;
        var new_max = 0.0;
        if (max_v > min_v) {
            new_mid = (mid_v - min_v) * sat / (max_v - min_v);
            new_max = sat;
        }
        out_r = set_channel_value(out_r, new_mid, mid_ix, 0u);
        out_g = set_channel_value(out_g, new_mid, mid_ix, 1u);
        out_b = set_channel_value(out_b, new_mid, mid_ix, 2u);
        out_r = set_channel_value(out_r, new_max, max_ix, 0u);
        out_g = set_channel_value(out_g, new_max, max_ix, 1u);
        out_b = set_channel_value(out_b, new_max, max_ix, 2u);
    }

    if (channel == 0u) {
        return out_r;
    } else if (channel == 1u) {
        return out_g;
    }
    return out_b;
}

fn channel_value(r: f32, g: f32, b: f32, channel: u32) -> f32 {
    if (channel == 0u) {
        return r;
    } else if (channel == 1u) {
        return g;
    }
    return b;
}

fn set_channel_value(current: f32, value: f32, src_channel: u32, dst_channel: u32) -> f32 {
    if (src_channel == dst_channel) {
        return value;
    }
    return current;
}

