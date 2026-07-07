fn apply_color_filter_pixel(px: u32, filter_kind: u32, amount: f32) -> u32 {
    let inv_255 = 1.0 / 255.0;
    var r = f32(px & 255u) * inv_255;
    var g = f32((px >> 8u) & 255u) * inv_255;
    var b = f32((px >> 16u) & 255u) * inv_255;
    var a = f32((px >> 24u) & 255u) * inv_255;

    if (filter_kind == FILTER_OPACITY) {
        let opacity = clamp(amount, 0.0, 1.0);
        r *= opacity;
        g *= opacity;
        b *= opacity;
        a *= opacity;
    } else if (a > 0.0) {
        let alpha = a;
        var ur = r / alpha;
        var ug = g / alpha;
        var ub = b / alpha;

        if (filter_kind == FILTER_BRIGHTNESS) {
            ur *= amount;
            ug *= amount;
            ub *= amount;
        } else if (filter_kind == FILTER_CONTRAST) {
            ur = (ur - 0.5) * amount + 0.5;
            ug = (ug - 0.5) * amount + 0.5;
            ub = (ub - 0.5) * amount + 0.5;
        } else if (filter_kind == FILTER_GRAYSCALE) {
            let t = clamp(amount, 0.0, 1.0);
            let l = svg_lum3(ur, ug, ub);
            ur = lerp_f32(ur, l, t);
            ug = lerp_f32(ug, l, t);
            ub = lerp_f32(ub, l, t);
        } else if (filter_kind == FILTER_HUE_ROTATE) {
            let angle = amount * 0.017453292;
            let co = cos(angle);
            let si = sin(angle);
            let nr = (0.213 + co * 0.787 - si * 0.213) * ur +
                (0.715 - co * 0.715 - si * 0.715) * ug +
                (0.072 - co * 0.072 + si * 0.928) * ub;
            let ng = (0.213 - co * 0.213 + si * 0.143) * ur +
                (0.715 + co * 0.285 + si * 0.140) * ug +
                (0.072 - co * 0.072 - si * 0.283) * ub;
            let nb = (0.213 - co * 0.213 - si * 0.787) * ur +
                (0.715 - co * 0.715 + si * 0.715) * ug +
                (0.072 + co * 0.928 + si * 0.072) * ub;
            ur = nr;
            ug = ng;
            ub = nb;
        } else if (filter_kind == FILTER_INVERT) {
            let t = clamp(amount, 0.0, 1.0);
            ur = lerp_f32(ur, 1.0 - ur, t);
            ug = lerp_f32(ug, 1.0 - ug, t);
            ub = lerp_f32(ub, 1.0 - ub, t);
        } else if (filter_kind == FILTER_SATURATE) {
            let l = svg_lum3(ur, ug, ub);
            ur = l + (ur - l) * amount;
            ug = l + (ug - l) * amount;
            ub = l + (ub - l) * amount;
        } else if (filter_kind == FILTER_SEPIA) {
            let t = clamp(amount, 0.0, 1.0);
            let sr = ur * 0.393 + ug * 0.769 + ub * 0.189;
            let sg = ur * 0.349 + ug * 0.686 + ub * 0.168;
            let sb = ur * 0.272 + ug * 0.534 + ub * 0.131;
            ur = lerp_f32(ur, sr, t);
            ug = lerp_f32(ug, sg, t);
            ub = lerp_f32(ub, sb, t);
        }

        r = clamp(ur, 0.0, 1.0) * alpha;
        g = clamp(ug, 0.0, 1.0) * alpha;
        b = clamp(ub, 0.0, 1.0) * alpha;
    }

    return pack_premul_rgba8(r, g, b, a);
}

fn apply_color_matrix_pixel(px: u32) -> u32 {
    let inv_255 = 1.0 / 255.0;
    let premul_r = f32(px & 255u) * inv_255;
    let premul_g = f32((px >> 8u) & 255u) * inv_255;
    let premul_b = f32((px >> 16u) & 255u) * inv_255;
    let alpha = f32((px >> 24u) & 255u) * inv_255;

    var r = 0.0;
    var g = 0.0;
    var b = 0.0;
    if (alpha > 0.0) {
        r = premul_r / alpha;
        g = premul_g / alpha;
        b = premul_b / alpha;
    }

    let rgba = vec4<f32>(r, g, b, alpha);
    let out_r = dot(config.matrix_r, rgba) + config.matrix_bias.x;
    let out_g = dot(config.matrix_g, rgba) + config.matrix_bias.y;
    let out_b = dot(config.matrix_b, rgba) + config.matrix_bias.z;
    let out_a = clamp(dot(config.matrix_a, rgba) + config.matrix_bias.w, 0.0, 1.0);
    return pack_premul_rgba8(
        clamp(out_r, 0.0, 1.0) * out_a,
        clamp(out_g, 0.0, 1.0) * out_a,
        clamp(out_b, 0.0, 1.0) * out_a,
        out_a,
    );
}

fn apply_component_transfer_pixel(px: u32, table_index: u32) -> u32 {
    let alpha = (px >> 24u) & 255u;
    let base = table_index * COMPONENT_TRANSFER_TABLE_LEN;
    let r_index = straight_component_index(px & 255u, alpha);
    let g_index = straight_component_index((px >> 8u) & 255u, alpha);
    let b_index = straight_component_index((px >> 16u) & 255u, alpha);
    let inv_255 = 1.0 / 255.0;
    let r = f32(transfer_tables[base + r_index]) * inv_255;
    let g = f32(transfer_tables[base + COMPONENT_TRANSFER_TABLE_SIZE + g_index]) * inv_255;
    let b = f32(transfer_tables[base + 2u * COMPONENT_TRANSFER_TABLE_SIZE + b_index]) * inv_255;
    let a = f32(transfer_tables[base + 3u * COMPONENT_TRANSFER_TABLE_SIZE + alpha]) * inv_255;
    return pack_premul_rgba8(r * a, g * a, b * a, a);
}

fn straight_component_index(premul: u32, alpha: u32) -> u32 {
    if (alpha == 0u) {
        return 0u;
    }
    return min((premul * 255u + alpha / 2u) / alpha, 255u);
}

fn source_alpha_at(x: u32, y: u32) -> f32 {
    return f32((source_pixel_at(x, y) >> 24u) & 255u) / 255.0;
}

fn filter_displacement_channel(px: u32, channel: u32, linear_rgb: u32) -> f32 {
    let alpha = (px >> 24u) & 255u;
    var value = f32(alpha) / 255.0;
    if (channel != 3u) {
        var premul = px & 255u;
        if (channel == 1u) {
            premul = (px >> 8u) & 255u;
        } else if (channel == 2u) {
            premul = (px >> 16u) & 255u;
        }
        value = straight_channel(premul, alpha);
        if (linear_rgb != 0u) {
            value = filter_srgb_to_linear(value);
        }
    }
    return value;
}

fn filter_srgb_to_linear(value: f32) -> f32 {
    var out = value / 12.92;
    if (value > 0.04045) {
        out = pow((value + 0.055) / 1.055, 2.4);
    }
    return out;
}

fn filter_linear_rgb_to_srgb(value: f32) -> f32 {
    var out = value * 12.92;
    if (value > 0.0031308) {
        out = 1.055 * pow(value, 1.0 / 2.4) - 0.055;
    }
    return out;
}

fn filter_turbulence_pixel(x: f32, y: f32) -> u32 {
    var result = 0u;
    if (
        abs(config.turbulence_scale_x) > 0.00000011920929 &&
        abs(config.turbulence_scale_y) > 0.00000011920929
    ) {
        let sample_base_x = (x - config.turbulence_transform_x) / config.turbulence_scale_x;
        let sample_base_y = (y - config.turbulence_transform_y) / config.turbulence_scale_y;
        let local_tile_x = x - config.turbulence_tile_x;
        let local_tile_y = y - config.turbulence_tile_y;
        var frequency_x = config.turbulence_base_frequency_x;
        var frequency_y = config.turbulence_base_frequency_y;
        var stitch_width = 0i;
        var stitch_height = 0i;
        var stitch_wrap_x = 0i;
        var stitch_wrap_y = 0i;
        if (config.turbulence_stitch_tiles == 1u) {
            let tw = max(config.turbulence_tile_width, 1.0);
            let th = max(config.turbulence_tile_height, 1.0);
            frequency_x = filter_stitch_frequency(frequency_x, tw);
            frequency_y = filter_stitch_frequency(frequency_y, th);
            stitch_width = i32(tw * frequency_x + 0.5);
            stitch_height = i32(th * frequency_y + 0.5);
            stitch_wrap_x = i32(local_tile_x * frequency_x + 4096.0 + f32(stitch_width));
            stitch_wrap_y = i32(local_tile_y * frequency_y + 4096.0 + f32(stitch_height));
        }

        let selector_offset = config.table_index * TURBULENCE_TABLE_LEN;
        let gradient_offset = config.table_index * TURBULENCE_GRADIENT_LEN;
        var ratio = 1.0;
        var out_r = 0.0;
        var out_g = 0.0;
        var out_b = 0.0;
        var out_a = 0.0;
        var octave = 0u;
        loop {
            if (octave >= config.turbulence_num_octaves) {
                break;
            }
            let sample_x = sample_base_x * frequency_x;
            let sample_y = sample_base_y * frequency_y;
            let r = filter_turbulence_noise2(0u, sample_x, sample_y, stitch_wrap_x, stitch_width, stitch_wrap_y, stitch_height, selector_offset, gradient_offset);
            let g = filter_turbulence_noise2(1u, sample_x, sample_y, stitch_wrap_x, stitch_width, stitch_wrap_y, stitch_height, selector_offset, gradient_offset);
            let b = filter_turbulence_noise2(2u, sample_x, sample_y, stitch_wrap_x, stitch_width, stitch_wrap_y, stitch_height, selector_offset, gradient_offset);
            let a = filter_turbulence_noise2(3u, sample_x, sample_y, stitch_wrap_x, stitch_width, stitch_wrap_y, stitch_height, selector_offset, gradient_offset);
            if (config.turbulence_kind == 0u) {
                out_r += abs(r) * ratio;
                out_g += abs(g) * ratio;
                out_b += abs(b) * ratio;
                out_a += abs(a) * ratio;
            } else {
                out_r += r * ratio;
                out_g += g * ratio;
                out_b += b * ratio;
                out_a += a * ratio;
            }
            frequency_x *= 2.0;
            frequency_y *= 2.0;
            ratio *= 0.5;
            if (config.turbulence_stitch_tiles == 1u) {
                stitch_width *= 2i;
                stitch_height *= 2i;
                stitch_wrap_x = 2i * stitch_wrap_x - 4096i;
                stitch_wrap_y = 2i * stitch_wrap_y - 4096i;
            }
            octave += 1u;
        }

        if (config.turbulence_kind == 1u) {
            out_r = out_r * 0.5 + 0.5;
            out_g = out_g * 0.5 + 0.5;
            out_b = out_b * 0.5 + 0.5;
            out_a = out_a * 0.5 + 0.5;
        }
        out_r = clamp(out_r, 0.0, 1.0);
        out_g = clamp(out_g, 0.0, 1.0);
        out_b = clamp(out_b, 0.0, 1.0);
        out_a = clamp(out_a, 0.0, 1.0);
        if (config.turbulence_linear_rgb == 1u) {
            out_r = filter_linear_rgb_to_srgb(out_r);
            out_g = filter_linear_rgb_to_srgb(out_g);
            out_b = filter_linear_rgb_to_srgb(out_b);
        }
        result = pack_premul_rgba8(out_r * out_a, out_g * out_a, out_b * out_a, out_a);
    }
    return result;
}

fn filter_stitch_frequency(frequency: f32, tile_size: f32) -> f32 {
    var out = 0.0;
    if (frequency > 0.0 && tile_size > 0.0) {
        let low = floor(tile_size * frequency) / tile_size;
        let high = ceil(tile_size * frequency) / tile_size;
        if (low != 0.0 && frequency / low < high / frequency) {
            out = low;
        } else {
            out = high;
        }
    }
    return out;
}

fn filter_turbulence_noise2(
    channel: u32,
    x: f32,
    y: f32,
    stitch_wrap_x: i32,
    stitch_width: i32,
    stitch_wrap_y: i32,
    stitch_height: i32,
    selector_offset: u32,
    gradient_offset: u32,
) -> f32 {
    let tx = x + 4096.0;
    let ty = y + 4096.0;
    var bx0 = i32(floor(tx));
    var bx1 = bx0 + 1i;
    var by0 = i32(floor(ty));
    var by1 = by0 + 1i;
    let rx0 = tx - f32(bx0);
    let rx1 = rx0 - 1.0;
    let ry0 = ty - f32(by0);
    let ry1 = ry0 - 1.0;
    if (config.turbulence_stitch_tiles == 1u) {
        if (bx0 >= stitch_wrap_x) {
            bx0 -= stitch_width;
        }
        if (bx1 >= stitch_wrap_x) {
            bx1 -= stitch_width;
        }
        if (by0 >= stitch_wrap_y) {
            by0 -= stitch_height;
        }
        if (by1 >= stitch_wrap_y) {
            by1 -= stitch_height;
        }
    }
    let ubx0 = u32(bx0 & 255i);
    let ubx1 = u32(bx1 & 255i);
    let uby0 = u32(by0 & 255i);
    let uby1 = u32(by1 & 255i);
    let ix = turbulence_selectors[selector_offset + ubx0];
    let jx = turbulence_selectors[selector_offset + ubx1];
    let b00 = turbulence_selectors[selector_offset + ix + uby0];
    let b10 = turbulence_selectors[selector_offset + jx + uby0];
    let b01 = turbulence_selectors[selector_offset + ix + uby1];
    let b11 = turbulence_selectors[selector_offset + jx + uby1];
    let sx = filter_turbulence_curve(rx0);
    let sy = filter_turbulence_curve(ry0);
    let a = lerp_f32(
        filter_turbulence_gradient_dot(gradient_offset, channel, b00, rx0, ry0),
        filter_turbulence_gradient_dot(gradient_offset, channel, b10, rx1, ry0),
        sx,
    );
    let b = lerp_f32(
        filter_turbulence_gradient_dot(gradient_offset, channel, b01, rx0, ry1),
        filter_turbulence_gradient_dot(gradient_offset, channel, b11, rx1, ry1),
        sx,
    );
    return lerp_f32(a, b, sy);
}

fn filter_turbulence_curve(t: f32) -> f32 {
    return t * t * (3.0 - 2.0 * t);
}

fn filter_turbulence_gradient_dot(
    gradient_offset: u32,
    channel: u32,
    selector: u32,
    x: f32,
    y: f32,
) -> f32 {
    let ix = gradient_offset + (channel * TURBULENCE_TABLE_LEN + selector) * 2u;
    return turbulence_gradients[ix] * x + turbulence_gradients[ix + 1u] * y;
}

fn liquid_glass_pixel(
    base: u32,
    world_x: f32,
    world_y: f32,
    pixel_x: f32,
    pixel_y: f32,
    distance: f32,
    distance_norm: f32,
    surface_height: f32,
) -> u32 {
    let normal = liquid_glass_normal(world_x, world_y);
    let nx = normal.x;
    let ny = normal.y;
    let inside_distance = -distance;
    let edge = liquid_glass_edge(
        inside_distance,
        config.liquid_refraction_thickness,
        config.liquid_refraction_factor,
    );
    var blur_mix = inside_distance / max(config.liquid_refraction_thickness, LIQUID_GLASS_EPSILON);
    if (config.mask_enabled == 1u) {
        blur_mix = 1.0;
    }
    blur_mix = clamp(blur_mix, 0.0, 1.0);

    let normal_len = LIQUID_GLASS_NORMAL_LENGTH_SCALE / surface_height;
    let initial_blur = liquid_glass_sample_straight_rgba(1u, pixel_x, pixel_y);
    var r = initial_blur.r;
    var g = initial_blur.g;
    var b = initial_blur.b;
    var a = initial_blur.a;
    let tint_mix = config.liquid_tint_a * LIQUID_GLASS_TINT_MIX;
    let tint_base_mix = config.liquid_tint_a * LIQUID_GLASS_TINT_BASE_MIX;

    if (edge <= 0.0) {
        if (tint_mix > 0.0) {
            r = lerp_f32(r, config.liquid_tint_r, tint_mix);
            g = lerp_f32(g, config.liquid_tint_g, tint_mix);
            b = lerp_f32(b, config.liquid_tint_b, tint_mix);
            a = lerp_f32(a, 1.0, tint_mix);
        }
    } else {
        let offset_x = -nx * edge * LIQUID_GLASS_REFRACTION_PIXEL_SCALE;
        let offset_y = -ny * edge * LIQUID_GLASS_REFRACTION_PIXEL_SCALE;
        if (abs(config.liquid_refraction_dispersion) <= LIQUID_GLASS_EPSILON) {
            let sx = pixel_x + offset_x;
            let sy = pixel_y + offset_y;
            let src_rgba = liquid_glass_sample_straight_rgba(0u, sx, sy);
            let blur_rgba = liquid_glass_sample_straight_rgba(1u, sx, sy);
            r = lerp_f32(src_rgba.r, blur_rgba.r, blur_mix);
            g = lerp_f32(src_rgba.g, blur_rgba.g, blur_mix);
            b = lerp_f32(src_rgba.b, blur_rgba.b, blur_mix);
            a = max(src_rgba.a, blur_rgba.a);
        } else {
            let sx = pixel_x + offset_x;
            let sy = pixel_y + offset_y;
            let src_rgba = liquid_glass_sample_straight_rgba(0u, sx, sy);
            let blur_rgba = liquid_glass_sample_straight_rgba(1u, sx, sy);
            r = liquid_glass_dispersion_channel(pixel_x, pixel_y, offset_x, offset_y, LIQUID_GLASS_CHROMATIC_R, 0u, blur_mix);
            g = lerp_f32(src_rgba.g, blur_rgba.g, blur_mix);
            b = liquid_glass_dispersion_channel(pixel_x, pixel_y, offset_x, offset_y, LIQUID_GLASS_CHROMATIC_B, 2u, blur_mix);
            a = max(src_rgba.a, blur_rgba.a);
        }
        let blurred_r = r;
        let blurred_g = g;
        let blurred_b = b;
        if (tint_mix > 0.0) {
            r = lerp_f32(r, config.liquid_tint_r, tint_mix);
            g = lerp_f32(g, config.liquid_tint_g, tint_mix);
            b = lerp_f32(b, config.liquid_tint_b, tint_mix);
            a = lerp_f32(a, 1.0, tint_mix);
        }

        if (config.liquid_fresnel_factor > 0.0) {
            let fresnel = liquid_glass_fresnel(distance, config.liquid_fresnel_range, config.liquid_fresnel_hardness);
            let fresnel_base_r = lerp_f32(1.0, config.liquid_tint_r, tint_base_mix);
            let fresnel_base_g = lerp_f32(1.0, config.liquid_tint_g, tint_base_mix);
            let fresnel_base_b = lerp_f32(1.0, config.liquid_tint_b, tint_base_mix);
            var fresnel_l = liquid_glass_srgb_to_lch_l(fresnel_base_r, fresnel_base_g, fresnel_base_b);
            let fresnel_c = liquid_glass_srgb_to_lch_c(fresnel_base_r, fresnel_base_g, fresnel_base_b);
            let fresnel_h = liquid_glass_srgb_to_lch_h(fresnel_base_r, fresnel_base_g, fresnel_base_b);
            fresnel_l = clamp(fresnel_l + LIQUID_GLASS_FRESNEL_LIGHTNESS_GAIN * fresnel * config.liquid_fresnel_factor, 0.0, 100.0);
            let fresnel_mix = fresnel * config.liquid_fresnel_factor * LIQUID_GLASS_FRESNEL_MIX_SCALE * normal_len;
            r = lerp_f32(r, liquid_glass_lch_to_srgb_r(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
            g = lerp_f32(g, liquid_glass_lch_to_srgb_g(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
            b = lerp_f32(b, liquid_glass_lch_to_srgb_b(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
            a = lerp_f32(a, 1.0, fresnel_mix);
        }

        if (config.liquid_glare_factor > 0.0) {
            let glare_geo = liquid_glass_glare_geometry(distance, config.liquid_glare_range, config.liquid_glare_hardness);
            let glare_angle_factor = liquid_glass_glare_angle(nx, ny);
            let glare_base_r = lerp_f32(blurred_r, config.liquid_tint_r, tint_base_mix);
            let glare_base_g = lerp_f32(blurred_g, config.liquid_tint_g, tint_base_mix);
            let glare_base_b = lerp_f32(blurred_b, config.liquid_tint_b, tint_base_mix);
            var glare_l = liquid_glass_srgb_to_lch_l(glare_base_r, glare_base_g, glare_base_b);
            var glare_c = liquid_glass_srgb_to_lch_c(glare_base_r, glare_base_g, glare_base_b);
            let glare_h = liquid_glass_srgb_to_lch_h(glare_base_r, glare_base_g, glare_base_b);
            glare_l = clamp(glare_l + LIQUID_GLASS_GLARE_LIGHTNESS_GAIN * glare_angle_factor * glare_geo, 0.0, 120.0);
            glare_c += LIQUID_GLASS_GLARE_CHROMA_GAIN * glare_angle_factor * glare_geo;
            let glare_mix = glare_angle_factor * glare_geo * normal_len;
            r = lerp_f32(r, liquid_glass_lch_to_srgb_r(glare_l, glare_c, glare_h), glare_mix);
            g = lerp_f32(g, liquid_glass_lch_to_srgb_g(glare_l, glare_c, glare_h), glare_mix);
            b = lerp_f32(b, liquid_glass_lch_to_srgb_b(glare_l, glare_c, glare_h), glare_mix);
            a = lerp_f32(a, 1.0, glare_mix);
        }
    }

    let edge_mix = liquid_glass_smoothstep(LIQUID_GLASS_EDGE_BLEND_START, LIQUID_GLASS_EDGE_BLEND_END, distance_norm);
    r = lerp_f32(r, liquid_glass_pixel_straight_channel(base, 0u), edge_mix);
    g = lerp_f32(g, liquid_glass_pixel_straight_channel(base, 1u), edge_mix);
    b = lerp_f32(b, liquid_glass_pixel_straight_channel(base, 2u), edge_mix);
    a = lerp_f32(a, liquid_glass_pixel_straight_channel(base, 3u), edge_mix);
    return liquid_glass_pack_straight_rgba8(r, g, b, a);
}

fn liquid_glass_edge(inside_distance: f32, refraction_thickness: f32, refraction_factor: f32) -> f32 {
    let thickness = max(refraction_thickness, LIQUID_GLASS_EPSILON);
    var out = 0.0;
    if (inside_distance < thickness) {
        let ratio = 1.0 - inside_distance / thickness;
        let theta_i = asin(clamp(pow(ratio, 2.0), -1.0, 1.0));
        let theta_t = asin(clamp(sin(theta_i) / max(refraction_factor, 1.0), -1.0, 1.0));
        out = max(-tan(theta_t - theta_i), 0.0);
    }
    return out;
}

fn liquid_glass_fresnel(distance: f32, fresnel_range: f32, fresnel_hardness: f32) -> f32 {
    return clamp(
        pow(
            1.0 + distance / LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE *
            pow(LIQUID_GLASS_GEOMETRY_RANGE_SCALE / max(fresnel_range, LIQUID_GLASS_EPSILON), 2.0) +
            fresnel_hardness,
            5.0,
        ),
        0.0,
        1.0,
    );
}

fn liquid_glass_glare_geometry(distance: f32, glare_range: f32, glare_hardness: f32) -> f32 {
    return clamp(
        pow(
            1.0 + distance / LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE *
            pow(LIQUID_GLASS_GEOMETRY_RANGE_SCALE / max(glare_range, LIQUID_GLASS_EPSILON), 2.0) +
            glare_hardness,
            5.0,
        ),
        0.0,
        1.0,
    );
}

fn liquid_glass_glare_angle(nx: f32, ny: f32) -> f32 {
    let angle = (liquid_glass_vec2_angle(nx, ny) - LIQUID_GLASS_PI * 0.25 + config.liquid_glare_angle) * 2.0;
    var side = LIQUID_GLASS_GLARE_SIDE_SCALE;
    if ((angle > LIQUID_GLASS_PI * 1.5 && angle < LIQUID_GLASS_PI * 3.5) || angle < -LIQUID_GLASS_PI * 0.5) {
        side = LIQUID_GLASS_GLARE_SIDE_SCALE * config.liquid_glare_opposite_factor;
    }
    return clamp(
        pow(
            (0.5 + sin(angle) * 0.5) * side * config.liquid_glare_factor,
            LIQUID_GLASS_GLARE_POWER_BASE + config.liquid_glare_convergence * LIQUID_GLASS_GLARE_POWER_SCALE,
        ),
        0.0,
        1.0,
    );
}

fn liquid_glass_vec2_angle(x: f32, y: f32) -> f32 {
    let len = sqrt(x * x + y * y);
    var angle = 0.0;
    if (len >= 0.00000001) {
        angle = atan2(y, x);
        if (angle < 0.0) {
            angle += 2.0 * LIQUID_GLASS_PI;
        }
    }
    return angle;
}

fn liquid_glass_dispersion_channel(
    x: f32,
    y: f32,
    offset_x: f32,
    offset_y: f32,
    chromatic: f32,
    channel: u32,
    blur_mix: f32,
) -> f32 {
    let factor = 1.0 - (chromatic - 1.0) * config.liquid_refraction_dispersion;
    let sx = x + offset_x * factor;
    let sy = y + offset_y * factor;
    let src = liquid_glass_sample_straight_channel(0u, sx, sy, channel);
    let blur = liquid_glass_sample_straight_channel(1u, sx, sy, channel);
    return lerp_f32(src, blur, blur_mix);
}

fn liquid_glass_sample_alpha(x: f32, y: f32) -> f32 {
    return max(
        liquid_glass_sample_straight_channel(0u, x, y, 3u),
        liquid_glass_sample_straight_channel(1u, x, y, 3u),
    );
}

fn liquid_glass_sample_straight_channel(image_kind: u32, x: f32, y: f32, channel: u32) -> f32 {
    let rgba = liquid_glass_sample_straight_rgba(image_kind, x, y);
    if (channel == 1u) {
        return rgba.g;
    }
    if (channel == 2u) {
        return rgba.b;
    }
    if (channel == 3u) {
        return rgba.a;
    }
    return rgba.r;
}

fn liquid_glass_sample_straight_rgba(image_kind: u32, x: f32, y: f32) -> vec4<f32> {
    if (image_kind == 1u && config.downsample > 1u) {
        return liquid_glass_sample_downsampled_blur_straight_rgba(x, y);
    }

    let sx = clamp(x, 0.0, f32(config.width - 1u));
    let sy = clamp(y, 0.0, f32(config.height - 1u));
    if (image_kind == 1u) {
        return liquid_glass_premul_to_straight_rgba(filter_aux_sample_premul(sx, sy));
    }
    return liquid_glass_premul_to_straight_rgba(filter_source_sample_premul(sx, sy));
}

fn liquid_glass_sample_downsampled_blur_straight_rgba(x: f32, y: f32) -> vec4<f32> {
    if (config.source_x0 >= config.source_x1 || config.source_y0 >= config.source_y1) {
        return vec4<f32>(0.0);
    }
    let sx = clamp(x, 0.0, f32(config.width - 1u));
    let sy = clamp(y, 0.0, f32(config.height - 1u));
    return liquid_glass_downsampled_blur_straight_rgba_at_full_res(sx, sy);
}

fn liquid_glass_sample_downsampled_blur_straight_channel(x: f32, y: f32, channel: u32) -> f32 {
    let rgba = liquid_glass_sample_downsampled_blur_straight_rgba(x, y);
    if (channel == 1u) {
        return rgba.g;
    }
    if (channel == 2u) {
        return rgba.b;
    }
    if (channel == 3u) {
        return rgba.a;
    }
    return rgba.r;
}

fn liquid_glass_downsampled_blur_straight_rgba_at_full_res(x: f32, y: f32) -> vec4<f32> {
    let factor = f32(max(config.downsample, 1u));
    let max_x = f32(config.source_x1 - 1u);
    let max_y = f32(config.source_y1 - 1u);
    let sample_x = clamp((x + 0.5) / factor - 0.5, f32(config.source_x0), max_x);
    let sample_y = clamp((y + 0.5) / factor - 0.5, f32(config.source_y0), max_y);
    if (config.upsample_filter == 0u) {
        return liquid_glass_pixel_straight_rgba(aux_pixel_at(u32(round(sample_x)), u32(round(sample_y))));
    }
    return liquid_glass_premul_to_straight_rgba(filter_aux_sample_premul(sample_x, sample_y));
}

fn liquid_glass_image_pixel(image_kind: u32, x: u32, y: u32) -> u32 {
    if (image_kind == 1u) {
        return aux_pixel_at(x, y);
    }
    return source_pixel_at(x, y);
}

fn liquid_glass_pixel_straight_rgba(px: u32) -> vec4<f32> {
    let a = f32((px >> 24u) & 255u) / 255.0;
    var r = f32(px & 255u) / 255.0;
    var g = f32((px >> 8u) & 255u) / 255.0;
    var b = f32((px >> 16u) & 255u) / 255.0;
    if (a > LIQUID_GLASS_EPSILON) {
        r = r / a;
        g = g / a;
        b = b / a;
    }
    return vec4<f32>(r, g, b, a);
}

fn liquid_glass_premul_to_straight_rgba(premul: vec4<f32>) -> vec4<f32> {
    var rgba = premul;
    if (rgba.a > LIQUID_GLASS_EPSILON) {
        rgba.r = rgba.r / rgba.a;
        rgba.g = rgba.g / rgba.a;
        rgba.b = rgba.b / rgba.a;
    }
    return rgba;
}

fn liquid_glass_pixel_straight_channel(px: u32, channel: u32) -> f32 {
    let rgba = liquid_glass_pixel_straight_rgba(px);
    if (channel == 1u) {
        return rgba.g;
    }
    if (channel == 2u) {
        return rgba.b;
    }
    if (channel == 3u) {
        return rgba.a;
    }
    return rgba.r;
}

fn liquid_glass_pack_straight_rgba8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    let alpha = clamp(a, 0.0, 1.0);
    return pack_premul_rgba8(
        clamp(r, 0.0, 1.0) * alpha,
        clamp(g, 0.0, 1.0) * alpha,
        clamp(b, 0.0, 1.0) * alpha,
        alpha,
    );
}

fn liquid_glass_smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn liquid_glass_normal(x: f32, y: f32) -> vec2<f32> {
    let eps = 1.0;
    let dx = liquid_glass_round_rect_distance(x + eps, y, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right) -
        liquid_glass_round_rect_distance(x - eps, y, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right);
    let dy = liquid_glass_round_rect_distance(x, y + eps, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right) -
        liquid_glass_round_rect_distance(x, y - eps, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right);
    let len = sqrt(dx * dx + dy * dy);
    if (len > LIQUID_GLASS_EPSILON) {
        return vec2<f32>(dx / len, dy / len);
    }
    return vec2<f32>(0.0, -1.0);
}

fn liquid_glass_round_rect_distance(
    x: f32,
    y: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
) -> f32 {
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = max((x1 - x0) * 0.5, 0.0);
    let hy = max((y1 - y0) * 0.5, 0.0);
    let px = x - cx;
    let py = y - cy;
    let radius = max(min(min(liquid_glass_corner_radius(px, py, radius_top_left, radius_top_right, radius_bottom_left, radius_bottom_right), hx), hy), 0.0);
    let ax = abs(px);
    let ay = abs(py);
    let dx = ax - hx;
    let dy = ay - hy;
    var out = sqrt(max(dx, 0.0) * max(dx, 0.0) + max(dy, 0.0) * max(dy, 0.0)) + min(max(dx, dy), 0.0);
    if (radius > 0.0) {
        let qx = ax - hx + radius;
        let qy = ay - hy + radius;
        out = min(max(qx, qy), 0.0) + sqrt(max(qx, 0.0) * max(qx, 0.0) + max(qy, 0.0) * max(qy, 0.0)) - radius;
    }
    return out;
}

fn liquid_glass_corner_radius(
    px: f32,
    py: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
) -> f32 {
    var radius = radius_top_left;
    if (px >= 0.0) {
        if (py <= 0.0) {
            radius = radius_top_right;
        } else {
            radius = radius_bottom_right;
        }
    } else if (py > 0.0) {
        radius = radius_bottom_left;
    }
    return radius;
}

fn liquid_glass_srgb_to_lch_l(r: f32, g: f32, b: f32) -> f32 {
    let y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    return 116.0 * y - 16.0;
}

fn liquid_glass_srgb_to_lch_c(r: f32, g: f32, b: f32) -> f32 {
    let lab_a = liquid_glass_srgb_to_lab_a(r, g, b);
    let lab_b = liquid_glass_srgb_to_lab_b(r, g, b);
    return sqrt(lab_a * lab_a + lab_b * lab_b);
}

fn liquid_glass_srgb_to_lch_h(r: f32, g: f32, b: f32) -> f32 {
    return atan2(liquid_glass_srgb_to_lab_b(r, g, b), liquid_glass_srgb_to_lab_a(r, g, b)) * 57.29578;
}

fn liquid_glass_srgb_to_lab_a(r: f32, g: f32, b: f32) -> f32 {
    let x = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_x(r, g, b) / LIQUID_GLASS_D65_X);
    let y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    return 500.0 * (x - y);
}

fn liquid_glass_srgb_to_lab_b(r: f32, g: f32, b: f32) -> f32 {
    let y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    let z = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_z(r, g, b) / LIQUID_GLASS_D65_Z);
    return 200.0 * (y - z);
}

fn liquid_glass_srgb_to_xyz_x(r: f32, g: f32, b: f32) -> f32 {
    return liquid_glass_uncompand_srgb(r) * 0.4124 + liquid_glass_uncompand_srgb(g) * 0.3576 + liquid_glass_uncompand_srgb(b) * 0.1805;
}

fn liquid_glass_srgb_to_xyz_y(r: f32, g: f32, b: f32) -> f32 {
    return liquid_glass_uncompand_srgb(r) * 0.2126 + liquid_glass_uncompand_srgb(g) * 0.7152 + liquid_glass_uncompand_srgb(b) * 0.0722;
}

fn liquid_glass_srgb_to_xyz_z(r: f32, g: f32, b: f32) -> f32 {
    return liquid_glass_uncompand_srgb(r) * 0.0193 + liquid_glass_uncompand_srgb(g) * 0.1192 + liquid_glass_uncompand_srgb(b) * 0.9505;
}

fn liquid_glass_lch_to_srgb_r(l: f32, c: f32, h: f32) -> f32 {
    let x = liquid_glass_lch_to_xyz_x(l, c, h);
    let y = liquid_glass_lch_to_xyz_y(l);
    let z = liquid_glass_lch_to_xyz_z(l, c, h);
    return liquid_glass_compand_rgb(x * 3.2406255 + y * -1.537208 + z * -0.4986286);
}

fn liquid_glass_lch_to_srgb_g(l: f32, c: f32, h: f32) -> f32 {
    let x = liquid_glass_lch_to_xyz_x(l, c, h);
    let y = liquid_glass_lch_to_xyz_y(l);
    let z = liquid_glass_lch_to_xyz_z(l, c, h);
    return liquid_glass_compand_rgb(x * -0.9689307 + y * 1.8757561 + z * 0.0415175);
}

fn liquid_glass_lch_to_srgb_b(l: f32, c: f32, h: f32) -> f32 {
    let x = liquid_glass_lch_to_xyz_x(l, c, h);
    let y = liquid_glass_lch_to_xyz_y(l);
    let z = liquid_glass_lch_to_xyz_z(l, c, h);
    return liquid_glass_compand_rgb(x * 0.0557101 + y * -0.2040211 + z * 1.0569959);
}

fn liquid_glass_lch_to_xyz_x(l: f32, c: f32, h: f32) -> f32 {
    let hue = h * 0.017453292;
    let lab_a = c * cos(hue);
    let w = (l + 16.0) / 116.0;
    return LIQUID_GLASS_D65_X * liquid_glass_lab_to_xyz_f(w + lab_a / 500.0);
}

fn liquid_glass_lch_to_xyz_y(l: f32) -> f32 {
    let w = (l + 16.0) / 116.0;
    return LIQUID_GLASS_D65_Y * liquid_glass_lab_to_xyz_f(w);
}

fn liquid_glass_lch_to_xyz_z(l: f32, c: f32, h: f32) -> f32 {
    let hue = h * 0.017453292;
    let lab_b = c * sin(hue);
    let w = (l + 16.0) / 116.0;
    return LIQUID_GLASS_D65_Z * liquid_glass_lab_to_xyz_f(w - lab_b / 200.0);
}

fn liquid_glass_xyz_to_lab_f(x: f32) -> f32 {
    var out = 7.787037 * x + 0.13793103;
    if (x > 0.008856452) {
        out = pow(x, 0.33333334);
    }
    return out;
}

fn liquid_glass_lab_to_xyz_f(x: f32) -> f32 {
    var out = 0.12841855 * (x - 0.13793103);
    if (x > 0.206897) {
        out = x * x * x;
    }
    return out;
}

fn liquid_glass_uncompand_srgb(a: f32) -> f32 {
    var out = a / 12.92;
    if (a > 0.04045) {
        out = pow((a + 0.055) / 1.055, 2.4);
    }
    return out;
}

fn liquid_glass_compand_rgb(a: f32) -> f32 {
    var out = 12.92 * a;
    if (a > 0.0031308) {
        out = 1.055 * pow(a, 0.41666666) - 0.055;
    }
    return out;
}

fn alpha_gradient_x(x: u32, y: u32) -> f32 {
    var out = 0.0;
    if (config.region_width >= 2u) {
        let weighted_diff = alpha_gradient_x_sample(x, y, -1, 1.0) +
            alpha_gradient_x_sample(x, y, 0, 2.0) +
            alpha_gradient_x_sample(x, y, 1, 1.0);
        let weight_sum = gradient_sample_weight(y, config.region_y0, config.region_height, -1, 1.0) +
            gradient_sample_weight(y, config.region_y0, config.region_height, 0, 2.0) +
            gradient_sample_weight(y, config.region_y0, config.region_height, 1, 1.0);
        let one_sided = x == config.region_x0 || x == config.region_x0 + config.region_width - 1u;
        var edge_scale = 1.0;
        if (one_sided) {
            edge_scale = 2.0;
        }
        out = weighted_diff * edge_scale / max(weight_sum, 0.000001);
    }
    return out;
}

fn alpha_gradient_y(x: u32, y: u32) -> f32 {
    var out = 0.0;
    if (config.region_height >= 2u) {
        let weighted_diff = alpha_gradient_y_sample(x, y, -1, 1.0) +
            alpha_gradient_y_sample(x, y, 0, 2.0) +
            alpha_gradient_y_sample(x, y, 1, 1.0);
        let weight_sum = gradient_sample_weight(x, config.region_x0, config.region_width, -1, 1.0) +
            gradient_sample_weight(x, config.region_x0, config.region_width, 0, 2.0) +
            gradient_sample_weight(x, config.region_x0, config.region_width, 1, 1.0);
        let one_sided = y == config.region_y0 || y == config.region_y0 + config.region_height - 1u;
        var edge_scale = 1.0;
        if (one_sided) {
            edge_scale = 2.0;
        }
        out = weighted_diff * edge_scale / max(weight_sum, 0.000001);
    }
    return out;
}

fn gradient_sample_weight(pos: u32, start: u32, len: u32, offset: i32, weight: f32) -> f32 {
    let sample = i32(pos) + offset;
    let end = start + len;
    var out = 0.0;
    if (sample >= i32(start) && sample < i32(end)) {
        out = weight;
    }
    return out;
}

fn alpha_gradient_x_sample(x: u32, y: u32, offset: i32, weight: f32) -> f32 {
    let sy = i32(y) + offset;
    let region_x1 = config.region_x0 + config.region_width - 1u;
    let region_y1 = config.region_y0 + config.region_height;
    var out = 0.0;
    if (sy >= i32(config.region_y0) && sy < i32(region_y1)) {
        let syu = u32(sy);
        var left = x;
        if (x > config.region_x0) {
            left = x - 1u;
        }
        var right = x;
        if (x < region_x1) {
            right = x + 1u;
        }
        let center = source_alpha_at(x, syu);
        var diff = source_alpha_at(right, syu) - source_alpha_at(left, syu);
        if (x == config.region_x0) {
            diff = source_alpha_at(right, syu) - center;
        } else if (x == region_x1) {
            diff = center - source_alpha_at(left, syu);
        }
        out = weight * diff;
    }
    return out;
}

fn alpha_gradient_y_sample(x: u32, y: u32, offset: i32, weight: f32) -> f32 {
    let sx = i32(x) + offset;
    let region_x1 = config.region_x0 + config.region_width;
    let region_y1 = config.region_y0 + config.region_height - 1u;
    var out = 0.0;
    if (sx >= i32(config.region_x0) && sx < i32(region_x1)) {
        let sxu = u32(sx);
        var top = y;
        if (y > config.region_y0) {
            top = y - 1u;
        }
        var bottom = y;
        if (y < region_y1) {
            bottom = y + 1u;
        }
        let center = source_alpha_at(sxu, y);
        var diff = source_alpha_at(sxu, bottom) - source_alpha_at(sxu, top);
        if (y == config.region_y0) {
            diff = source_alpha_at(sxu, bottom) - center;
        } else if (y == region_y1) {
            diff = center - source_alpha_at(sxu, top);
        }
        out = weight * diff;
    }
    return out;
}

fn composite_inputs_pixel(input1: u32, input2: u32, composite_operator: u32, k1: f32, k2: f32, k3: f32, k4: f32) -> u32 {
    var out = blend_premul_u8(input2, input1, 3u << 8u);
    if (composite_operator == 1u) {
        out = blend_premul_u8(input2, input1, 5u << 8u);
    } else if (composite_operator == 2u) {
        out = blend_premul_u8(input2, input1, 7u << 8u);
    } else if (composite_operator == 3u) {
        out = blend_premul_u8(input2, input1, 9u << 8u);
    } else if (composite_operator == 4u) {
        out = blend_premul_u8(input2, input1, 11u << 8u);
    } else if (composite_operator == 5u) {
        out = arithmetic_composite_pixel(input1, input2, k1, k2, k3, k4);
    }
    return out;
}

fn arithmetic_composite_pixel(input1: u32, input2: u32, k1: f32, k2: f32, k3: f32, k4: f32) -> u32 {
    let inv = 1.0 / 255.0;
    let a_r = f32(input1 & 255u) * inv;
    let a_g = f32((input1 >> 8u) & 255u) * inv;
    let a_b = f32((input1 >> 16u) & 255u) * inv;
    let a_a = f32((input1 >> 24u) & 255u) * inv;
    let b_r = f32(input2 & 255u) * inv;
    let b_g = f32((input2 >> 8u) & 255u) * inv;
    let b_b = f32((input2 >> 16u) & 255u) * inv;
    let b_a = f32((input2 >> 24u) & 255u) * inv;
    return pack_premul_rgba8(
        arithmetic_channel(a_r, b_r, k1, k2, k3, k4),
        arithmetic_channel(a_g, b_g, k1, k2, k3, k4),
        arithmetic_channel(a_b, b_b, k1, k2, k3, k4),
        arithmetic_channel(a_a, b_a, k1, k2, k3, k4),
    );
}

fn arithmetic_channel(a: f32, b: f32, k1: f32, k2: f32, k3: f32, k4: f32) -> f32 {
    return clamp(k1 * a * b + k2 * a + k3 * b + k4, 0.0, 1.0);
}

fn svg_lum3(r: f32, g: f32, b: f32) -> f32 {
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    return a + (b - a) * t;
}

fn lerp_vec4(a: vec4<f32>, b: vec4<f32>, t: f32) -> vec4<f32> {
    return a + (b - a) * t;
}

@compute @workgroup_size(256)
fn filter_apply_region_mask(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    let alpha = combine_alpha(target_load_ix(ix) >> 24u, aux_pixel_ix(ix) >> 24u);
    target_store_ix(ix, gray_alpha(alpha));
}

