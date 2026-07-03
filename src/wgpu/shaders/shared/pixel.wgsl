fn coverage_to_u8(coverage: f32) -> u32 {
    return u32(clamp(coverage, 0.0, 1.0) * 255.0 + 0.5);
}

fn coverage_to_alpha(value: f32, fill_rule: u32) -> u32 {
    var alpha = min(abs(value), 1.0);
    if (fill_rule == 1u) {
        alpha = abs(value - 2.0 * round(0.5 * value));
    }
    return coverage_to_u8(alpha);
}

fn signum_f32(value: f32) -> f32 {
    var out = 1.0;
    if (value < 0.0) {
        out = -1.0;
    }
    return out;
}

fn rgba8_pack(r: u32, g: u32, b: u32, a: u32) -> u32 {
    return r | (g << 8u) | (b << 16u) | (a << 24u);
}

fn unorm_to_rgba8(pixel: vec4<f32>) -> u32 {
    return rgba8_pack(
        u32(clamp(pixel.r * 255.0 + 0.5, 0.0, 255.0)),
        u32(clamp(pixel.g * 255.0 + 0.5, 0.0, 255.0)),
        u32(clamp(pixel.b * 255.0 + 0.5, 0.0, 255.0)),
        u32(clamp(pixel.a * 255.0 + 0.5, 0.0, 255.0)),
    );
}

fn rgba8_to_unorm(pixel: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(pixel & 255u) * (1.0 / 255.0),
        f32((pixel >> 8u) & 255u) * (1.0 / 255.0),
        f32((pixel >> 16u) & 255u) * (1.0 / 255.0),
        f32((pixel >> 24u) & 255u) * (1.0 / 255.0),
    );
}

fn mul_div255(a: u32, b: u32) -> u32 {
    let t = a * b + 128u;
    return (t + (t >> 8u)) >> 8u;
}

fn combine_alpha(a: u32, b: u32) -> u32 {
    return mul_div255(a, b);
}

fn straight_channel(premul: u32, alpha: u32) -> f32 {
    if (alpha == 0u) {
        return 0.0;
    }
    return f32(premul) / f32(alpha);
}

fn scale_premul_u8(src: u32, factor: u32) -> u32 {
    if (factor == 0u) {
        return 0u;
    }
    if (factor == 255u) {
        return src;
    }
    return rgba8_pack(
        mul_div255(src & 255u, factor),
        mul_div255((src >> 8u) & 255u, factor),
        mul_div255((src >> 16u) & 255u, factor),
        mul_div255((src >> 24u) & 255u, factor),
    );
}

fn src_over_premul_u8(dst: u32, src: u32) -> u32 {
    let sa = (src >> 24u) & 255u;
    if (sa == 0u) {
        return dst;
    }
    if (sa == 255u) {
        return src;
    }
    let inv = 255u - sa;
    return rgba8_pack(
        (src & 255u) + mul_div255(dst & 255u, inv),
        ((src >> 8u) & 255u) + mul_div255((dst >> 8u) & 255u, inv),
        ((src >> 16u) & 255u) + mul_div255((dst >> 16u) & 255u, inv),
        sa + mul_div255((dst >> 24u) & 255u, inv),
    );
}

fn pack_premul_rgba8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    return u32(clamp(r, 0.0, 1.0) * 255.0 + 0.5) |
        (u32(clamp(g, 0.0, 1.0) * 255.0 + 0.5) << 8u) |
        (u32(clamp(b, 0.0, 1.0) * 255.0 + 0.5) << 16u) |
        (u32(clamp(a, 0.0, 1.0) * 255.0 + 0.5) << 24u);
}

fn lerp_premul_u8(a: u32, b: u32, t: f32) -> u32 {
    let inv = 1.0 / 255.0;
    let ar = f32(a & 255u) * inv;
    let ag = f32((a >> 8u) & 255u) * inv;
    let ab = f32((a >> 16u) & 255u) * inv;
    let aa = f32((a >> 24u) & 255u) * inv;
    let br = f32(b & 255u) * inv;
    let bg = f32((b >> 8u) & 255u) * inv;
    let bb = f32((b >> 16u) & 255u) * inv;
    let ba = f32((b >> 24u) & 255u) * inv;
    return rgba8_pack(
        u32(clamp(ar + (br - ar) * t, 0.0, 1.0) * 255.0 + 0.5),
        u32(clamp(ag + (bg - ag) * t, 0.0, 1.0) * 255.0 + 0.5),
        u32(clamp(ab + (bb - ab) * t, 0.0, 1.0) * 255.0 + 0.5),
        u32(clamp(aa + (ba - aa) * t, 0.0, 1.0) * 255.0 + 0.5),
    );
}

fn rem_euclid_f32(value: f32, modulus: f32) -> f32 {
    return value - floor(value / modulus) * modulus;
}
