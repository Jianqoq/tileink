fn unpack(px: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(px & 255u),
        f32((px >> 8u) & 255u),
        f32((px >> 16u) & 255u),
        f32(px >> 24u),
    ) / 255.0;
}

fn pack(c: vec4<f32>) -> u32 {
    let v = vec4<u32>(clamp(c, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + 0.5);
    return v.x | (v.y << 8u) | (v.z << 16u) | (v.w << 24u);
}

fn over(dst: vec4<f32>, src: vec4<f32>) -> vec4<f32> {
    return src + dst * (1.0 - src.w);
}

fn overlay_channel(dst: f32, src: f32) -> f32 {
    if dst <= 0.5 {
        return 2.0 * dst * src;
    }
    return 1.0 - 2.0 * (1.0 - dst) * (1.0 - src);
}

fn soft_light_channel(dst: f32, src: f32) -> f32 {
    if src <= 0.5 {
        return dst - (1.0 - 2.0 * src) * dst * (1.0 - dst);
    }
    var d = sqrt(dst);
    if dst <= 0.25 {
        d = ((16.0 * dst - 12.0) * dst + 4.0) * dst;
    }
    return dst + (2.0 * src - 1.0) * (d - dst);
}

fn blend_lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.3, 0.59, 0.11));
}

fn blend_sat(c: vec3<f32>) -> f32 {
    return max(c.x, max(c.y, c.z)) - min(c.x, min(c.y, c.z));
}

fn clip_color(input: vec3<f32>) -> vec3<f32> {
    var c = input;
    let l = blend_lum(c);
    let n = min(c.x, min(c.y, c.z));
    let x = max(c.x, max(c.y, c.z));
    if n < 0.0 {
        c = vec3<f32>(
            l + (c.x - l) * l / (l - n),
            l + (c.y - l) * l / (l - n),
            l + (c.z - l) * l / (l - n),
        );
    }
    if x > 1.0 {
        c = vec3<f32>(
            l + (c.x - l) * (1.0 - l) / (x - l),
            l + (c.y - l) * (1.0 - l) / (x - l),
            l + (c.z - l) * (1.0 - l) / (x - l),
        );
    }
    return c;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return clip_color(c + vec3<f32>(l - blend_lum(c)));
}

fn set_sat_inner(cmin: f32, cmid: f32, cmax: f32, s: f32) -> vec3<f32> {
    if cmax > cmin {
        return vec3<f32>(0.0, (cmid - cmin) * s / (cmax - cmin), s);
    }
    return vec3<f32>(0.0);
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    if c.x <= c.y && c.x <= c.z {
        if c.y <= c.z {
            let v = set_sat_inner(c.x, c.y, c.z, s);
            return vec3<f32>(v.x, v.y, v.z);
        }
        let v = set_sat_inner(c.x, c.z, c.y, s);
        return vec3<f32>(v.x, v.z, v.y);
    }
    if c.y <= c.z {
        if c.x <= c.z {
            let v = set_sat_inner(c.y, c.x, c.z, s);
            return vec3<f32>(v.y, v.x, v.z);
        }
        let v = set_sat_inner(c.y, c.z, c.x, s);
        return vec3<f32>(v.z, v.x, v.y);
    }
    if c.x <= c.y {
        let v = set_sat_inner(c.z, c.x, c.y, s);
        return vec3<f32>(v.y, v.z, v.x);
    }
    let v = set_sat_inner(c.z, c.y, c.x, s);
    return vec3<f32>(v.z, v.y, v.x);
}

fn blend_mix(dst: vec3<f32>, src: vec3<f32>, mode: u32) -> vec3<f32> {
    if mode == 0u {
        return src;
    }
    if mode == 1u {
        return dst * src;
    }
    if mode == 2u {
        return dst + src - dst * src;
    }
    if mode == 3u {
        return vec3<f32>(
            overlay_channel(dst.x, src.x),
            overlay_channel(dst.y, src.y),
            overlay_channel(dst.z, src.z),
        );
    }
    if mode == 4u {
        return min(dst, src);
    }
    if mode == 5u {
        return max(dst, src);
    }
    if mode == 6u {
        return min(vec3<f32>(1.0), dst / max(vec3<f32>(1e-7), vec3<f32>(1.0) - src));
    }
    if mode == 7u {
        return vec3<f32>(1.0) -
            min(vec3<f32>(1.0), (vec3<f32>(1.0) - dst) / max(vec3<f32>(1e-7), src));
    }
    if mode == 8u {
        return vec3<f32>(
            overlay_channel(src.x, dst.x),
            overlay_channel(src.y, dst.y),
            overlay_channel(src.z, dst.z),
        );
    }
    if mode == 9u {
        return vec3<f32>(
            soft_light_channel(dst.x, src.x),
            soft_light_channel(dst.y, src.y),
            soft_light_channel(dst.z, src.z),
        );
    }
    if mode == 10u {
        return abs(dst - src);
    }
    if mode == 11u {
        return dst + src - 2.0 * dst * src;
    }
    if mode == 12u {
        return set_lum(set_sat(src, blend_sat(dst)), blend_lum(dst));
    }
    if mode == 13u {
        return set_lum(set_sat(dst, blend_sat(src)), blend_lum(dst));
    }
    if mode == 14u {
        return set_lum(src, blend_lum(dst));
    }
    return set_lum(dst, blend_lum(src));
}

fn compose_factors(compose: u32, sa: f32, da: f32) -> vec2<f32> {
    if compose == 0u { return vec2<f32>(0.0, 0.0); }
    if compose == 1u { return vec2<f32>(1.0, 0.0); }
    if compose == 2u { return vec2<f32>(0.0, 1.0); }
    if compose == 3u { return vec2<f32>(1.0, 1.0 - sa); }
    if compose == 4u { return vec2<f32>(1.0 - da, 1.0); }
    if compose == 5u { return vec2<f32>(da, 0.0); }
    if compose == 6u { return vec2<f32>(0.0, sa); }
    if compose == 7u { return vec2<f32>(1.0 - da, 0.0); }
    if compose == 8u { return vec2<f32>(0.0, 1.0 - sa); }
    if compose == 9u { return vec2<f32>(da, 1.0 - sa); }
    if compose == 10u { return vec2<f32>(1.0 - da, sa); }
    if compose == 11u { return vec2<f32>(1.0 - da, 1.0 - sa); }
    return vec2<f32>(1.0, 1.0);
}
