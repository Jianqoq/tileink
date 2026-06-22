struct Params {
    width: u32, height: u32, dst_width: u32, dst_height: u32,
    src_off: u32, dst_off: u32, aux_off: u32, op: u32,
    kind: u32, radius: u32, amount: f32, offset_x: i32,
    offset_y: i32, brush_off: u32,
}
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read_write> arena: array<atomic<u32>>;

fn load(off: u32) -> u32 { return atomicLoad(&arena[off >> 2u]); }
fn load_f32(off: u32) -> f32 { return bitcast<f32>(load(off)); }
fn store(off: u32, v: u32) { atomicStore(&arena[off >> 2u], v); }
fn unpack_u8_f32(px: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(px & 255u),
        f32((px >> 8u) & 255u),
        f32((px >> 16u) & 255u),
        f32(px >> 24u),
    );
}
fn pack_u8_f32(c: vec4<f32>) -> u32 {
    let v = vec4<u32>(clamp(c, vec4<f32>(0.0), vec4<f32>(255.0)) + 0.5);
    return v.x | (v.y << 8u) | (v.z << 16u) | (v.w << 24u);
}
fn lum(c: vec3<f32>) -> f32 { return dot(c,vec3<f32>(0.2126,0.7152,0.0722)); }
fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { return v / 12.92; }
    return pow((v + 0.055) / 1.055, 2.4);
}
fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 { return v * 12.92; }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}
fn in_dst_bounds(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && x < i32(p.dst_width) && y < i32(p.dst_height);
}
fn rem_euclid_i32(value: i32, modulus: i32) -> i32 {
    let rem = value % modulus;
    return select(rem + modulus, rem, rem >= 0);
}
fn target_ix(id: vec3<u32>) -> u32 {
    return (u32(i32(id.y)+p.offset_y)*p.dst_width)+u32(i32(id.x)+p.offset_x);
}
fn convert_color_space(px: vec4<f32>) -> vec4<f32> {
    if px.w <= 0.0 { return px; }
    var c = px.xyz / px.w;
    if p.kind == 0u {
        c = vec3<f32>(srgb_to_linear(c.x), srgb_to_linear(c.y), srgb_to_linear(c.z));
    } else {
        c = vec3<f32>(linear_to_srgb(c.x), linear_to_srgb(c.y), linear_to_srgb(c.z));
    }
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * px.w, px.w);
}
fn load_matrix(row: u32, col: u32) -> f32 {
    return load_f32(p.aux_off + (row * 5u + col) * 4u);
}
fn mul_div255_u8(a: u32, b: u32) -> u32 {
    return (a * b + 127u) / 255u;
}
fn premul_to_unpremul_u8(rgb: u32, a: u32) -> u32 {
    if a == 0u { return 0u; }
    return min((rgb * 255u + a / 2u) / a, 255u);
}
fn unpremul(px: vec4<f32>) -> vec4<f32> {
    if px.w <= 0.0 { return vec4<f32>(0.0); }
    return vec4<f32>(px.xyz / px.w, px.w);
}
fn premul(px: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(px.xyz * px.w, px.w);
}
fn color_matrix(px: vec4<f32>) -> vec4<f32> {
    let up = unpremul(px);
    let r = up.x;
    let g = up.y;
    let b = up.z;
    let a = up.w;
    let out = vec4<f32>(
        clamp(r * load_matrix(0u, 0u) + g * load_matrix(0u, 1u) + b * load_matrix(0u, 2u) + a * load_matrix(0u, 3u) + load_matrix(0u, 4u), 0.0, 1.0),
        clamp(r * load_matrix(1u, 0u) + g * load_matrix(1u, 1u) + b * load_matrix(1u, 2u) + a * load_matrix(1u, 3u) + load_matrix(1u, 4u), 0.0, 1.0),
        clamp(r * load_matrix(2u, 0u) + g * load_matrix(2u, 1u) + b * load_matrix(2u, 2u) + a * load_matrix(2u, 3u) + load_matrix(2u, 4u), 0.0, 1.0),
        clamp(r * load_matrix(3u, 0u) + g * load_matrix(3u, 1u) + b * load_matrix(3u, 2u) + a * load_matrix(3u, 3u) + load_matrix(3u, 4u), 0.0, 1.0)
    );
    let uq = vec4<u32>(clamp(out, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0);
    return vec4<f32>(
        f32(mul_div255_u8(uq.x, uq.w)) / 255.0,
        f32(mul_div255_u8(uq.y, uq.w)) / 255.0,
        f32(mul_div255_u8(uq.z, uq.w)) / 255.0,
        f32(uq.w) / 255.0
    );
}
fn transfer_lookup(channel: u32, value: u32) -> f32 {
    return f32(load(p.aux_off + (channel * 256u + value) * 4u) & 255u) / 255.0;
}
fn convolve_header_u32(index: u32) -> u32 {
    return load(p.aux_off + index * 4u);
}
fn convolve_header_f32(index: u32) -> f32 {
    return load_f32(p.aux_off + index * 4u);
}
fn convolve_matrix_value(columns: u32, rows: u32, ox: u32, oy: u32) -> f32 {
    let flipped_x = columns - ox - 1u;
    let flipped_y = rows - oy - 1u;
    return load_f32(p.aux_off + 32u + (flipped_y * columns + flipped_x) * 4u);
}
fn convolve_sample_coord(coord: i32, limit: i32, edge_mode: u32) -> i32 {
    if edge_mode == 1u {
        return clamp(coord, 0, limit - 1);
    }
    if edge_mode == 2u {
        return ((coord % limit) + limit) % limit;
    }
    return coord;
}
fn to_u8_round(value: f32) -> u32 {
    return u32(clamp(value, 0.0, 1.0) * 255.0 + 0.5);
}
fn lighting_u32(index: u32) -> u32 {
    return load(p.aux_off + index * 4u);
}
fn lighting_f32(index: u32) -> f32 {
    return load_f32(p.aux_off + index * 4u);
}
fn safe_normalize(v: vec3<f32>) -> vec3<f32> {
    let len = length(v);
    if len <= 1e-6 {
        return v;
    }
    return v / len;
}
fn safe_pow_nonnegative(base: f32, exponent: f32) -> f32 {
    if base <= 0.0 {
        if exponent < 0.0 {
            return 1e30;
        }
        if exponent == 0.0 {
            return 1.0;
        }
        return 0.0;
    }
    return pow(base, exponent);
}
fn alpha_at(x: u32, y: u32) -> f32 {
    return f32((load(p.src_off + (y * p.width + x) * 4u) >> 24u) & 255u);
}
fn lighting_normal(id: vec3<u32>) -> vec4<f32> {
    let x = id.x;
    let y = id.y;
    let max_x = p.width - 1u;
    let max_y = p.height - 1u;
    if x == 0u && y == 0u {
        return vec4<f32>(
            2.0 / 3.0,
            2.0 / 3.0,
            -2.0 * alpha_at(0u, 0u) + 2.0 * alpha_at(1u, 0u) - alpha_at(0u, 1u) + alpha_at(1u, 1u),
            -2.0 * alpha_at(0u, 0u) - alpha_at(1u, 0u) + 2.0 * alpha_at(0u, 1u) + alpha_at(1u, 1u),
        );
    }
    if x == max_x && y == 0u {
        return vec4<f32>(
            2.0 / 3.0,
            2.0 / 3.0,
            -2.0 * alpha_at(max_x - 1u, 0u) + 2.0 * alpha_at(max_x, 0u) - alpha_at(max_x - 1u, 1u) + alpha_at(max_x, 1u),
            -alpha_at(max_x - 1u, 0u) - 2.0 * alpha_at(max_x, 0u) + alpha_at(max_x - 1u, 1u) + 2.0 * alpha_at(max_x, 1u),
        );
    }
    if x == 0u && y == max_y {
        return vec4<f32>(
            2.0 / 3.0,
            2.0 / 3.0,
            -alpha_at(0u, max_y - 1u) + alpha_at(1u, max_y - 1u) - 2.0 * alpha_at(0u, max_y) + 2.0 * alpha_at(1u, max_y),
            -2.0 * alpha_at(0u, max_y - 1u) - alpha_at(1u, max_y - 1u) + 2.0 * alpha_at(0u, max_y) + alpha_at(1u, max_y),
        );
    }
    if x == max_x && y == max_y {
        return vec4<f32>(
            2.0 / 3.0,
            2.0 / 3.0,
            -alpha_at(max_x - 1u, max_y - 1u) + alpha_at(max_x, max_y - 1u) - 2.0 * alpha_at(max_x - 1u, max_y) + 2.0 * alpha_at(max_x, max_y),
            -alpha_at(max_x - 1u, max_y - 1u) - 2.0 * alpha_at(max_x, max_y - 1u) + alpha_at(max_x - 1u, max_y) + 2.0 * alpha_at(max_x, max_y),
        );
    }
    if y == 0u {
        return vec4<f32>(
            1.0 / 3.0,
            1.0 / 2.0,
            -2.0 * alpha_at(x - 1u, 0u) + 2.0 * alpha_at(x + 1u, 0u) - alpha_at(x - 1u, 1u) + alpha_at(x + 1u, 1u),
            -alpha_at(x - 1u, 0u) - 2.0 * alpha_at(x, 0u) - alpha_at(x + 1u, 0u) + alpha_at(x - 1u, 1u) + 2.0 * alpha_at(x, 1u) + alpha_at(x + 1u, 1u),
        );
    }
    if y == max_y {
        return vec4<f32>(
            1.0 / 3.0,
            1.0 / 2.0,
            -alpha_at(x - 1u, max_y - 1u) + alpha_at(x + 1u, max_y - 1u) - 2.0 * alpha_at(x - 1u, max_y) + 2.0 * alpha_at(x + 1u, max_y),
            -alpha_at(x - 1u, max_y - 1u) - 2.0 * alpha_at(x, max_y - 1u) - alpha_at(x + 1u, max_y - 1u) + alpha_at(x - 1u, max_y) + 2.0 * alpha_at(x, max_y) + alpha_at(x + 1u, max_y),
        );
    }
    if x == 0u {
        return vec4<f32>(
            1.0 / 2.0,
            1.0 / 3.0,
            -alpha_at(0u, y - 1u) + alpha_at(1u, y - 1u) - 2.0 * alpha_at(0u, y) + 2.0 * alpha_at(1u, y) - alpha_at(0u, y + 1u) + alpha_at(1u, y + 1u),
            -2.0 * alpha_at(0u, y - 1u) - alpha_at(1u, y - 1u) + 2.0 * alpha_at(0u, y + 1u) + alpha_at(1u, y + 1u),
        );
    }
    if x == max_x {
        return vec4<f32>(
            1.0 / 2.0,
            1.0 / 3.0,
            -alpha_at(max_x - 1u, y - 1u) + alpha_at(max_x, y - 1u) - 2.0 * alpha_at(max_x - 1u, y) + 2.0 * alpha_at(max_x, y) - alpha_at(max_x - 1u, y + 1u) + alpha_at(max_x, y + 1u),
            -alpha_at(max_x - 1u, y - 1u) - 2.0 * alpha_at(max_x, y - 1u) + alpha_at(max_x - 1u, y + 1u) + 2.0 * alpha_at(max_x, y + 1u),
        );
    }
    return vec4<f32>(
        1.0 / 4.0,
        1.0 / 4.0,
        -alpha_at(x - 1u, y - 1u) + alpha_at(x + 1u, y - 1u) - 2.0 * alpha_at(x - 1u, y) + 2.0 * alpha_at(x + 1u, y) - alpha_at(x - 1u, y + 1u) + alpha_at(x + 1u, y + 1u),
        -alpha_at(x - 1u, y - 1u) - 2.0 * alpha_at(x, y - 1u) - alpha_at(x + 1u, y - 1u) + alpha_at(x - 1u, y + 1u) + 2.0 * alpha_at(x, y + 1u) + alpha_at(x + 1u, y + 1u),
    );
}
fn lighting_vector(id: vec3<u32>, light_kind: u32, surface_scale: f32, light_base: vec3<f32>) -> vec3<f32> {
    if light_kind == 0u {
        return light_base;
    }
    let z = alpha_at(id.x, id.y) / 255.0 * surface_scale;
    return safe_normalize(light_base - vec3<f32>(f32(id.x), f32(id.y), z));
}
fn spot_lighting_color(
    lighting_color: vec3<f32>,
    light_vector: vec3<f32>,
    light_base: vec3<f32>,
    light_target: vec3<f32>,
    spot_exponent: f32,
    has_cone_angle: bool,
    cone_cos: f32,
) -> vec3<f32> {
    let direction = safe_normalize(light_target - light_base);
    let minus_l_dot_s = -dot(light_vector, direction);
    if minus_l_dot_s <= 0.0 {
        return vec3<f32>(0.0);
    }
    if has_cone_angle && minus_l_dot_s < cone_cos {
        return vec3<f32>(0.0);
    }
    return lighting_color * safe_pow_nonnegative(minus_l_dot_s, spot_exponent);
}
fn svg_lighting(id: vec3<u32>) -> u32 {
    if p.width < 3u || p.height < 3u {
        return 0u;
    }
    let mode = lighting_u32(0u);
    let light_kind = lighting_u32(1u);
    let has_cone_angle = lighting_u32(2u) != 0u;
    let surface_scale = lighting_f32(4u);
    let constant = lighting_f32(5u);
    let primitive_exponent = lighting_f32(6u);
    let spot_exponent = lighting_f32(7u);
    let cone_cos = lighting_f32(8u);
    let lighting_color = vec3<f32>(lighting_f32(12u), lighting_f32(13u), lighting_f32(14u));
    let light_base = vec3<f32>(lighting_f32(16u), lighting_f32(17u), lighting_f32(18u));
    let light_target = vec3<f32>(lighting_f32(20u), lighting_f32(21u), lighting_f32(22u));
    let normal = lighting_normal(id);
    let light = lighting_vector(id, light_kind, surface_scale, light_base);
    let color = select(
        lighting_color,
        spot_lighting_color(
            lighting_color,
            light,
            light_base,
            light_target,
            spot_exponent,
            has_cone_angle,
            cone_cos,
        ),
        light_kind == 2u,
    );
    var factor = 0.0;
    let normal_is_zero = abs(normal.z) <= 1e-6 && abs(normal.w) <= 1e-6;
    if mode == 0u {
        if normal_is_zero {
            factor = light.z;
        } else {
            let nx = -normal.z * (surface_scale / 255.0) * normal.x;
            let ny = -normal.w * (surface_scale / 255.0) * normal.y;
            let n = vec3<f32>(nx, ny, 1.0);
            factor = dot(n, light) / length(n);
        }
        factor = constant * factor;
        let rgb = vec3<u32>(
            to_u8_round(color.x * factor),
            to_u8_round(color.y * factor),
            to_u8_round(color.z * factor),
        );
        return rgb.x | (rgb.y << 8u) | (rgb.z << 16u) | (255u << 24u);
    }
    let h = light + vec3<f32>(0.0, 0.0, 1.0);
    let h_length = length(h);
    if h_length <= 1e-6 {
        return 0u;
    }
    if normal_is_zero {
        factor = h.z / h_length;
    } else {
        let nx = -normal.z * (surface_scale / 255.0) * normal.x;
        let ny = -normal.w * (surface_scale / 255.0) * normal.y;
        let n = vec3<f32>(nx, ny, 1.0);
        factor = dot(n, h) / length(n) / h_length;
    }
    factor = constant * safe_pow_nonnegative(max(factor, 0.0), primitive_exponent);
    let rgb = vec3<u32>(
        to_u8_round(color.x * factor),
        to_u8_round(color.y * factor),
        to_u8_round(color.z * factor),
    );
    let alpha = max(rgb.x, max(rgb.y, rgb.z));
    return rgb.x | (rgb.y << 8u) | (rgb.z << 16u) | (alpha << 24u);
}
fn component_transfer(px: vec4<f32>) -> vec4<f32> {
    let rgba = vec4<u32>(clamp(px, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + 0.5);
    let up = vec4<u32>(
        premul_to_unpremul_u8(rgba.x, rgba.w),
        premul_to_unpremul_u8(rgba.y, rgba.w),
        premul_to_unpremul_u8(rgba.z, rgba.w),
        rgba.w
    );
    let out = vec4<u32>(
        u32(transfer_lookup(0u, up.x) * 255.0),
        u32(transfer_lookup(1u, up.y) * 255.0),
        u32(transfer_lookup(2u, up.z) * 255.0),
        u32(transfer_lookup(3u, up.w) * 255.0)
    );
    return vec4<f32>(
        f32(mul_div255_u8(out.x, out.w)) / 255.0,
        f32(mul_div255_u8(out.y, out.w)) / 255.0,
        f32(mul_div255_u8(out.z, out.w)) / 255.0,
        f32(out.w) / 255.0
    );
}
fn arithmetic_composite(src_px: u32, dst_px: u32) -> u32 {
    let k1 = load_f32(p.kind + 0u);
    let k2 = load_f32(p.kind + 4u);
    let k3 = load_f32(p.kind + 8u);
    let k4 = load_f32(p.kind + 12u);
    let src = vec4<f32>(
        f32(src_px & 255u),
        f32((src_px >> 8u) & 255u),
        f32((src_px >> 16u) & 255u),
        f32(src_px >> 24u),
    ) / 255.0;
    let dst = vec4<f32>(
        f32(dst_px & 255u),
        f32((dst_px >> 8u) & 255u),
        f32((dst_px >> 16u) & 255u),
        f32(dst_px >> 24u),
    ) / 255.0;
    let alpha = clamp(
        k1 * src.w * dst.w + k2 * src.w + k3 * dst.w + k4,
        0.0,
        1.0,
    );
    if alpha <= 0.0 {
        return 0u;
    }
    let out = vec4<u32>(
        u32(clamp(k1 * src.x * dst.x + k2 * src.x + k3 * dst.x + k4, 0.0, alpha) * 255.0),
        u32(clamp(k1 * src.y * dst.y + k2 * src.y + k3 * dst.y + k4, 0.0, alpha) * 255.0),
        u32(clamp(k1 * src.z * dst.z + k2 * src.z + k3 * dst.z + k4, 0.0, alpha) * 255.0),
        u32(alpha * 255.0),
    );
    return out.x | (out.y << 8u) | (out.z << 16u) | (out.w << 24u);
}
fn blend_pixel(dst: vec4<f32>, src: vec4<f32>, mode: u32, compose: u32) -> vec4<f32> {
    if mode==0u && compose==3u { return over(dst,src); }
    let sa=clamp(src.w,0.0,1.0); let da=clamp(dst.w,0.0,1.0);
    var sr=vec3<f32>(0.0); var dr=vec3<f32>(0.0);
    if sa>0.0 { sr=src.xyz/sa; }
    if da>0.0 { dr=dst.xyz/da; }
    var effective=src;
    if mode!=0u {
        let mixed=blend_mix(dr,sr,mode);
        effective=vec4<f32>(sa*((1.0-da)*sr+da*mixed),sa);
    }
    let f=compose_factors(compose,sa,da);
    var out=effective*f.x+dst*f.y;
    if compose>=12u { out=min(out,vec4<f32>(1.0)); }
    return out;
}
fn color_filter(px: vec4<f32>) -> vec4<f32> {
    if p.kind==5u { return px*clamp(p.amount,0.0,1.0); }
    if px.w<=0.0 { return px; }
    var c=px.xyz/px.w;
    if p.kind==0u { c*=p.amount; }
    else if p.kind==1u { c=(c-vec3<f32>(0.5))*p.amount+vec3<f32>(0.5); }
    else if p.kind==2u { let a=clamp(p.amount,0.0,1.0); c=mix(c,vec3<f32>(lum(c)),a); }
    else if p.kind==3u {
        let a=radians(p.amount); let co=cos(a); let si=sin(a);
        let m=mat3x3<f32>(
            vec3<f32>(0.213+co*0.787-si*0.213,0.213-co*0.213+si*0.143,0.213-co*0.213-si*0.787),
            vec3<f32>(0.715-co*0.715-si*0.715,0.715+co*0.285+si*0.140,0.715-co*0.715+si*0.715),
            vec3<f32>(0.072-co*0.072+si*0.928,0.072-co*0.072-si*0.283,0.072+co*0.928+si*0.072));
        c=m*c;
    } else if p.kind==4u { let a=clamp(p.amount,0.0,1.0); c=mix(c,vec3<f32>(1.0)-c,a); }
    else if p.kind==6u { let g=lum(c); c=vec3<f32>(g)+(c-vec3<f32>(g))*p.amount; }
    else {
        let a=clamp(p.amount,0.0,1.0);
        let s=vec3<f32>(dot(c,vec3<f32>(0.393,0.769,0.189)),dot(c,vec3<f32>(0.349,0.686,0.168)),dot(c,vec3<f32>(0.272,0.534,0.131)));
        c=mix(c,s,a);
    }
    return vec4<f32>(clamp(c,vec3<f32>(0.0),vec3<f32>(1.0))*px.w,px.w);
}
fn blur_weight(index: u32) -> f32 {
    return load_f32(p.aux_off + index * 4u);
}
fn brush_load_u32(off: u32) -> u32 { return load(off); }
fn brush_load_f32(off: u32) -> f32 { return load_f32(off); }
fn brush_unpack(v: u32) -> vec4<f32> { return unpack(v); }
fn brush_pack(v: vec4<f32>) -> u32 { return pack(v); }
const TURB_PARAMS_SIZE: u32 = 40u;
const TURB_LATTICE_OFF: u32 = TURB_PARAMS_SIZE;
const TURB_B_LEN: u32 = 514u;
const TURB_GRADIENT_OFF: u32 = TURB_LATTICE_OFF + TURB_B_LEN * 4u;
const TURB_PERLIN_N: f32 = 4096.0;
fn turb_f32(index: u32) -> f32 {
    return load_f32(p.aux_off + index * 4u);
}
fn turb_u32(index: u32) -> u32 {
    return load(p.aux_off + index * 4u);
}
fn turb_lattice(index: u32) -> u32 {
    return load(p.aux_off + TURB_LATTICE_OFF + index * 4u);
}
fn turb_gradient(channel: u32, index: u32) -> vec2<f32> {
    let base = p.aux_off + TURB_GRADIENT_OFF + ((channel * TURB_B_LEN + index) * 2u) * 4u;
    return vec2<f32>(load_f32(base), load_f32(base + 4u));
}
fn turb_s_curve(t: f32) -> f32 {
    return t * t * (3.0 - 2.0 * t);
}
fn turb_lerp(t: f32, a: f32, b: f32) -> f32 {
    return a + t * (b - a);
}
fn turb_noise2(channel: u32, x: f32, y: f32, stitch: vec4<i32>, stitched: bool) -> f32 {
    let tx = x + TURB_PERLIN_N;
    var bx0 = i32(tx);
    var bx1 = bx0 + 1;
    let rx0 = tx - f32(bx0);
    let rx1 = rx0 - 1.0;
    let ty = y + TURB_PERLIN_N;
    var by0 = i32(ty);
    var by1 = by0 + 1;
    let ry0 = ty - f32(by0);
    let ry1 = ry0 - 1.0;
    if stitched {
        if bx0 >= stitch.z { bx0 -= stitch.x; }
        if bx1 >= stitch.z { bx1 -= stitch.x; }
        if by0 >= stitch.w { by0 -= stitch.y; }
        if by1 >= stitch.w { by1 -= stitch.y; }
    }
    bx0 = bx0 & 255;
    bx1 = bx1 & 255;
    by0 = by0 & 255;
    by1 = by1 & 255;
    let i = turb_lattice(u32(bx0));
    let j = turb_lattice(u32(bx1));
    let b00 = turb_lattice(i + u32(by0));
    let b10 = turb_lattice(j + u32(by0));
    let b01 = turb_lattice(i + u32(by1));
    let b11 = turb_lattice(j + u32(by1));
    let sx = turb_s_curve(rx0);
    let sy = turb_s_curve(ry0);
    let q00 = turb_gradient(channel, b00);
    let u00 = rx0 * q00.x + ry0 * q00.y;
    let q10 = turb_gradient(channel, b10);
    let v10 = rx1 * q10.x + ry0 * q10.y;
    let a = turb_lerp(sx, u00, v10);
    let q01 = turb_gradient(channel, b01);
    let u01 = rx0 * q01.x + ry1 * q01.y;
    let q11 = turb_gradient(channel, b11);
    let v11 = rx1 * q11.x + ry1 * q11.y;
    return turb_lerp(sy, a, turb_lerp(sx, u01, v11));
}
fn turbulence_channel(channel: u32, tx: f32, ty: f32, tile_x: f32, tile_y: f32, tile_w: f32, tile_h: f32) -> u32 {
    var base_freq_x = turb_f32(0u);
    var base_freq_y = turb_f32(1u);
    let num_octaves = turb_u32(6u);
    let fractal_sum = turb_u32(7u) != 0u;
    let stitch_tiles = turb_u32(8u) != 0u;
    var stitch = vec4<i32>(0);
    var stitched = false;
    if stitch_tiles {
        if abs(base_freq_x) > 1e-6 {
            let lo = floor(tile_w * base_freq_x) / tile_w;
            let hi = ceil(tile_w * base_freq_x) / tile_w;
            base_freq_x = select(hi, lo, base_freq_x / lo < hi / base_freq_x);
        }
        if abs(base_freq_y) > 1e-6 {
            let lo = floor(tile_h * base_freq_y) / tile_h;
            let hi = ceil(tile_h * base_freq_y) / tile_h;
            base_freq_y = select(hi, lo, base_freq_y / lo < hi / base_freq_y);
        }
        let width = i32(tile_w * base_freq_x + 0.5);
        let height = i32(tile_h * base_freq_y + 0.5);
        stitch = vec4<i32>(
            width,
            height,
            i32(tile_x * base_freq_x + TURB_PERLIN_N + f32(width)),
            i32(tile_y * base_freq_y + TURB_PERLIN_N + f32(height))
        );
        stitched = true;
    }
    var x = tx * base_freq_x;
    var y = ty * base_freq_y;
    var ratio = 1.0;
    var sum = 0.0;
    for (var octave = 0u; octave < num_octaves; octave++) {
        let noise = turb_noise2(channel, x, y, stitch, stitched);
        if fractal_sum {
            sum += noise / ratio;
        } else {
            sum += abs(noise) / ratio;
        }
        x *= 2.0;
        y *= 2.0;
        ratio *= 2.0;
        if stitched {
            stitch.x *= 2;
            stitch.y *= 2;
            stitch.z = 2 * stitch.z - 4096;
            stitch.w = 2 * stitch.w - 4096;
        }
    }
    let value = select(sum * 255.0, (sum * 255.0 + 255.0) * 0.5, fractal_sum);
    return u32(clamp(value, 0.0, 255.0) + 0.5);
}
fn turbulence_pixel(id: vec3<u32>) -> u32 {
    let scale_x = turb_f32(4u);
    let scale_y = turb_f32(5u);
    let offset_x = turb_f32(2u);
    let offset_y = turb_f32(3u);
    let tx = (f32(id.x) + offset_x) / scale_x;
    let ty = (f32(id.y) + offset_y) / scale_y;
    let r = turbulence_channel(0u, tx, ty, f32(id.x), f32(id.y), f32(p.width), f32(p.height));
    let g = turbulence_channel(1u, tx, ty, f32(id.x), f32(id.y), f32(p.width), f32(p.height));
    let b = turbulence_channel(2u, tx, ty, f32(id.x), f32(id.y), f32(p.width), f32(p.height));
    let a = turbulence_channel(3u, tx, ty, f32(id.x), f32(id.y), f32(p.width), f32(p.height));
    return mul_div255_u8(r, a) | (mul_div255_u8(g, a) << 8u) | (mul_div255_u8(b, a) << 16u) | (a << 24u);
}
@compute @workgroup_size(16,16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x>=p.width || id.y>=p.height { return; }
    let ix=id.y*p.width+id.x; let dst=p.dst_off+ix*4u;
    if p.op==0u { store(dst,pack(color_filter(unpack(load(p.src_off+ix*4u))))); return; }
    if p.op==1u || p.op==2u {
        var acc=vec4<f32>(0.0); let r=i32(p.radius); let scale=1.0/f32(2u*p.radius+1u);
        for(var d=-r;d<=r;d++){
            var x=i32(id.x); var y=i32(id.y); if p.op==1u{x+=d;}else{y+=d;}
            if x>=0&&y>=0&&x<i32(p.width)&&y<i32(p.height){
                acc+=unpack_u8_f32(load(p.src_off+(u32(y)*p.width+u32(x))*4u));
            }
        }
        store(dst,pack_u8_f32(acc*scale)); return;
    }
    if p.op==18u || p.op==19u {
        var acc=vec4<f32>(0.0); let r=i32(p.radius);
        for(var d=-r;d<=r;d++){
            let w=blur_weight(u32(d+r));
            var x=i32(id.x); var y=i32(id.y); if p.op==18u{x+=d;}else{y+=d;}
            if x>=0&&y>=0&&x<i32(p.width)&&y<i32(p.height){
                acc+=unpack_u8_f32(load(p.src_off+(u32(y)*p.width+u32(x))*4u))*w;
            }
        }
        store(dst,pack_u8_f32(acc)); return;
    }
    if p.op==3u {
        let tx = i32(id.x) + p.offset_x;
        let ty = i32(id.y) + p.offset_y;
        if !in_dst_bounds(tx, ty) { return; }
        let t=p.dst_off+target_ix(id)*4u;
        store(t,pack(over(unpack(load(t)),unpack(load(p.src_off+ix*4u))))); return;
    }
    if p.op==4u {
        let src=unpack(load(p.src_off+ix*4u));
        store(dst,pack(vec4<f32>(0.0,0.0,0.0,src.w))); return;
    }
    if p.op==6u {
        let original=unpack(load(dst));
        let filtered=unpack(load(p.src_off+ix*4u));
        let coverage=clamp(unpack(load(p.aux_off+ix*4u)).w,0.0,1.0);
        store(dst,pack(mix(original,filtered,coverage))); return;
    }
    if p.op==7u { store(dst,load(p.src_off+ix*4u)); return; }
    if p.op==10u {
        let sx = i32(id.x) + p.offset_x;
        let sy = i32(id.y) + p.offset_y;
        if sx < 0 || sy < 0 || sx >= i32(p.kind) || sy >= i32(p.radius) {
            store(dst, 0u);
            return;
        }
        let src_ix = u32(sy) * p.kind + u32(sx);
        store(dst, load(p.src_off + src_ix * 4u));
        return;
    }
    if p.op==11u {
        store(dst, pack(convert_color_space(unpack(load(p.src_off + ix * 4u)))));
        return;
    }
    if p.op==12u {
        store(dst, pack(color_matrix(unpack(load(p.src_off + ix * 4u)))));
        return;
    }
    if p.op==13u {
        store(dst, pack(component_transfer(unpack(load(p.src_off + ix * 4u)))));
        return;
    }
    if p.op==14u {
        let tile_w = i32(p.kind);
        let tile_h = i32(p.radius);
        if tile_w <= 0 || tile_h <= 0 {
            store(dst, 0u);
            return;
        }
        let wrap_x = p.offset_x + rem_euclid_i32(i32(id.x) - p.offset_x, tile_w);
        let wrap_y = p.offset_y + rem_euclid_i32(i32(id.y) - p.offset_y, tile_h);
        store(dst, load(p.src_off + (u32(wrap_y) * p.width + u32(wrap_x)) * 4u));
        return;
    }
    if p.op==15u {
        let src_px = load(p.src_off + ix * 4u);
        let dst_px = load(p.aux_off + ix * 4u);
        store(dst, arithmetic_composite(src_px, dst_px));
        return;
    }
    if p.op==16u {
        let columns = convolve_header_u32(0u);
        let rows = convolve_header_u32(1u);
        let target_x = i32(convolve_header_u32(2u));
        let target_y = i32(convolve_header_u32(3u));
        let edge_mode = convolve_header_u32(4u);
        let preserve_alpha = convolve_header_u32(5u) != 0u;
        let divisor = convolve_header_f32(6u);
        let bias = convolve_header_f32(7u);
        let width = i32(p.width);
        let height = i32(p.height);
        let center = unpack(load(p.src_off + ix * 4u));
        let center_alpha = center.w;
        var acc = vec4<f32>(0.0);
        for (var oy = 0u; oy < rows; oy++) {
            for (var ox = 0u; ox < columns; ox++) {
                var sx = i32(id.x) - target_x + i32(ox);
                var sy = i32(id.y) - target_y + i32(oy);
                if edge_mode == 0u && (sx < 0 || sy < 0 || sx >= width || sy >= height) {
                    continue;
                }
                sx = convolve_sample_coord(sx, width, edge_mode);
                sy = convolve_sample_coord(sy, height, edge_mode);
                let sample = unpack(load(p.src_off + (u32(sy) * p.width + u32(sx)) * 4u));
                let sample_channels = select(sample, unpremul(sample), preserve_alpha);
                let k = convolve_matrix_value(columns, rows, ox, oy);
                acc.x += sample_channels.x * k;
                acc.y += sample_channels.y * k;
                acc.z += sample_channels.z * k;
                if !preserve_alpha {
                    acc.w += sample_channels.w * k;
                }
            }
        }
        let raw_alpha = select(acc.w / divisor + bias, center_alpha, preserve_alpha);
        let alpha = clamp(raw_alpha, 0.0, 1.0);
        let resolved_r = acc.x / divisor + bias * raw_alpha;
        let resolved_g = acc.y / divisor + bias * raw_alpha;
        let resolved_b = acc.z / divisor + bias * raw_alpha;
        let out_r = to_u8_round(select(
            clamp(resolved_r, 0.0, alpha),
            clamp(resolved_r, 0.0, 1.0) * alpha,
            preserve_alpha,
        ));
        let out_g = to_u8_round(select(
            clamp(resolved_g, 0.0, alpha),
            clamp(resolved_g, 0.0, 1.0) * alpha,
            preserve_alpha,
        ));
        let out_b = to_u8_round(select(
            clamp(resolved_b, 0.0, alpha),
            clamp(resolved_b, 0.0, 1.0) * alpha,
            preserve_alpha,
        ));
        store(dst, out_r | (out_g << 8u) | (out_b << 16u) | (to_u8_round(alpha) << 24u));
        return;
    }
    if p.op==17u {
        store(dst, svg_lighting(id));
        return;
    }
    if p.op==20u {
        let columns = p.radius;
        let rows = p.brush_off;
        if columns == 0u || rows == 0u {
            store(dst, 0u);
            return;
        }
        let target_x = i32(columns / 2u);
        let target_y = i32(rows / 2u);
        var out = select(vec4<u32>(0u), vec4<u32>(255u), p.kind == 0u);
        for (var oy = 0u; oy < rows; oy++) {
            for (var ox = 0u; ox < columns; ox++) {
                let sx = i32(id.x) - target_x + i32(ox);
                let sy = i32(id.y) - target_y + i32(oy);
                if sx < 0 || sy < 0 || sx >= i32(p.width) || sy >= i32(p.height) {
                    continue;
                }
                let sample = load(p.src_off + (u32(sy) * p.width + u32(sx)) * 4u);
                let rgba = vec4<u32>(
                    sample & 255u,
                    (sample >> 8u) & 255u,
                    (sample >> 16u) & 255u,
                    sample >> 24u,
                );
                if p.kind == 0u {
                    out = min(out, rgba);
                } else {
                    out = max(out, rgba);
                }
            }
        }
        store(dst, out.x | (out.y << 8u) | (out.z << 16u) | (out.w << 24u));
        return;
    }
    if p.op==21u {
        store(dst, turbulence_pixel(id));
        return;
    }
    if p.op==8u {
        let tx = i32(id.x) + p.offset_x;
        let ty = i32(id.y) + p.offset_y;
        if !in_dst_bounds(tx, ty) { return; }
        let content=unpack(load(p.src_off+ix*4u));
        let mask=unpack(load(p.aux_off+ix*4u));
        var coverage=mask.w;
        if p.kind==1u {
            if mask.w<=0.0 {
                coverage=0.0;
            } else {
                coverage=mask.w*lum(mask.xyz/mask.w);
            }
        }
        let t=p.dst_off+target_ix(id)*4u;
        store(t,pack(over(unpack(load(t)),content*clamp(coverage,0.0,1.0))));
        return;
    }
    if p.op==9u {
        let tx = i32(id.x) + p.offset_x;
        let ty = i32(id.y) + p.offset_y;
        if !in_dst_bounds(tx, ty) { return; }
        let t=p.dst_off+target_ix(id)*4u;
        store(t,pack(blend_pixel(unpack(load(t)),unpack(load(p.src_off+ix*4u)),p.kind,p.radius)));
        return;
    }
    let sx=i32(id.x)-p.offset_x; let sy=i32(id.y)-p.offset_y; var shadow=vec4<f32>(0.0);
    if sx>=0&&sy>=0&&sx<i32(p.width)&&sy<i32(p.height){
        let mask=unpack(load(p.aux_off+(u32(sy)*p.width+u32(sx))*4u)).w;
        shadow=unpack(sample_brush_u32(p.brush_off,vec2<f32>(f32(id.x)+0.5,f32(id.y)+0.5)))*mask;
    }
    store(dst,pack(over(shadow,unpack(load(p.src_off+ix*4u)))));
}
