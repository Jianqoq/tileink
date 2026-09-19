fn filter_svg_mask_coverage_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    let px = source_pixel_ix(ix);
    let a = px >> 24u;
    var mask_alpha = a;
    if (config.mask_kind == SVG_MASK_LUMINANCE) {
        var safe_a = a;
        if (safe_a == 0u) {
            safe_a = 1u;
        }
        let r = px & 255u;
        let g = (px >> 8u) & 255u;
        let b = (px >> 16u) & 255u;
        let straight_r = (r * 255u + safe_a / 2u) / safe_a;
        let straight_g = (g * 255u + safe_a / 2u) / safe_a;
        let straight_b = (b * 255u + safe_a / 2u) / safe_a;
        mask_alpha = ((2126u * straight_r + 7152u * straight_g + 722u * straight_b) * a + 1275000u) / 2550000u;
    }
    target_store_ix(ix, gray_alpha(mask_alpha));
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_color_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, apply_color_filter_pixel(target_load_ix(ix), config.filter_kind, config.amount));
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_color_matrix_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, apply_color_matrix_pixel(target_load_ix(ix)));
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_component_transfer_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, apply_component_transfer_pixel(target_load_ix(ix), config.table_index));
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_blend_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, blend_premul_u8(aux_pixel_ix(ix), source_pixel_ix(ix), config.blend_mode));
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_composite_inputs_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, composite_inputs_pixel(
        source_pixel_ix(ix),
        aux_pixel_ix(ix),
        config.filter_kind,
        config.matrix_bias.x,
        config.matrix_bias.y,
        config.matrix_bias.z,
        config.matrix_bias.w,
    ));
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_displacement_map_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    let map = aux_pixel_ix(ix);
    let dx = filter_displacement_channel(map, config.kernel_edge_mode, config.lighting_output_kind) - 0.5;
    let dy = filter_displacement_channel(map, config.kernel_preserve_alpha, config.lighting_output_kind) - 0.5;
    // Match native coordinate fusion before rounding at half-pixel boundaries.
    let sx = round(fma(dx, config.amount, f32(xy.x)));
    let sy = round(fma(dy, config.rect_x0, f32(xy.y)));
    var out = 0u;
    // Test floating bounds before conversion, including overflow from finite scales.
    if (sx >= 0.0 && sx < f32(config.width) && sy >= 0.0 && sy < f32(config.height)) {
        out = source_pixel_at(u32(sx), u32(sy));
    }
    target_store_ix(ix, out);
}

// Exponent-bit scaling prevents fast-math from constructing 1/subnormal.
fn convolve_scale_24(value: f32) -> f32 {
    let bits = bitcast<u32>(value);
    let magnitude = bits & 0x7fffffffu;
    let exponent = magnitude >> 23u;
    if (exponent == 0u) {
        let scaled = f32(magnitude) * bitcast<f32>(0x01000000u);
        return select(scaled, -scaled, (bits & 0x80000000u) != 0u);
    }
    if (exponent >= 231u) { return bitcast<f32>((bits & 0x80000000u) | 0x7f800000u); }
    return bitcast<f32>(bits + (24u << 23u));
}

// Preserve a visible quotient when a large divisor's reciprocal would flush.
fn convolve_unscale_24(value: f32) -> f32 {
    let bits = bitcast<u32>(value);
    let exponent = (bits & 0x7fffffffu) >> 23u;
    if (exponent <= 24u) { return 0.0; }
    return bitcast<f32>(bits - (24u << 23u));
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_convolve_matrix_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let dst_ix = target_ix_at(xy.x, xy.y);
    let divisor = config.amount;
    let divisor_magnitude = bitcast<u32>(divisor) & 0x7fffffffu;
    if (config.kernel_columns == 0u || config.kernel_rows == 0u || divisor_magnitude == 0u) {
        target_store_ix(dst_ix, source_pixel_ix(dst_ix));
        return;
    }

    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    var out_r = 0.0;
    var out_g = 0.0;
    var out_b = 0.0;
    var out_a = 0.0;
    var ky = 0u;
    loop {
        if (ky >= config.kernel_rows) {
            break;
        }
        var kx = 0u;
        loop {
            if (kx >= config.kernel_columns) {
                break;
            }
            let kernel_ix = config.kernel_offset +
                (config.kernel_rows - 1u - ky) * config.kernel_columns +
                (config.kernel_columns - 1u - kx);
            let weight = convolve_kernels[kernel_ix];
            var sx = i32(xy.x) + i32(kx) - i32(config.kernel_target_x);
            var sy = i32(xy.y) + i32(ky) - i32(config.kernel_target_y);
            var sample = 0u;
            if (config.kernel_edge_mode == 1u) {
                sx = clamp(sx, i32(config.region_x0), region_x1 - 1);
                sy = clamp(sy, i32(config.region_y0), region_y1 - 1);
                sample = source_pixel_at(u32(sx), u32(sy));
            } else if (config.kernel_edge_mode == 2u) {
                while (sx < i32(config.region_x0)) {
                    sx = sx + i32(config.region_width);
                }
                while (sx >= region_x1) {
                    sx = sx - i32(config.region_width);
                }
                while (sy < i32(config.region_y0)) {
                    sy = sy + i32(config.region_height);
                }
                while (sy >= region_y1) {
                    sy = sy - i32(config.region_height);
                }
                sample = source_pixel_at(u32(sx), u32(sy));
            } else if (
                sx >= i32(config.region_x0) &&
                sx < region_x1 &&
                sy >= i32(config.region_y0) &&
                sy < region_y1
            ) {
                sample = source_pixel_at(u32(sx), u32(sy));
            }

            let alpha = (sample >> 24u) & 255u;
            out_r = fma(straight_channel(sample & 255u, alpha), weight, out_r);
            out_g = fma(straight_channel((sample >> 8u) & 255u, alpha), weight, out_g);
            out_b = fma(straight_channel((sample >> 16u) & 255u, alpha), weight, out_b);
            out_a = fma(f32(alpha), weight, out_a);
            kx += 1u;
        }
        ky += 1u;
    }

    // Match native convolution's byte-domain alpha and explicit fused order.
    // Avoid normalizing every tap then rescaling at a half-byte boundary.
    let reciprocal = 1.0 / divisor;
    var alpha: f32;
    var straight: vec3<f32>;
    let sum = vec3<f32>(out_r, out_g, out_b);
    if (divisor_magnitude < 0x00800000u) {
        // Decode the subnormal divisor and scale both sides by 2^24 before FTZ.
        var scaled_divisor = f32(divisor_magnitude) * bitcast<f32>(0x01000000u);
        if ((bitcast<u32>(divisor) & 0x80000000u) != 0u) { scaled_divisor = -scaled_divisor; }
        straight = clamp(vec3<f32>(convolve_scale_24(out_r), convolve_scale_24(out_g), convolve_scale_24(out_b)) / scaled_divisor + vec3<f32>(config.rect_x0), vec3<f32>(0.0), vec3<f32>(1.0));
        alpha = clamp(convolve_scale_24(out_a) / (scaled_divisor * 255.0) + config.rect_x0, 0.0, 1.0) * 255.0;
    } else if (divisor_magnitude > 0x7e800000u) {
        let inverse = 1.0 / convolve_unscale_24(divisor);
        let reduced = vec3<f32>(convolve_unscale_24(out_r), convolve_unscale_24(out_g), convolve_unscale_24(out_b));
        straight = clamp(fma(reduced, vec3<f32>(inverse), vec3<f32>(config.rect_x0)), vec3<f32>(0.0), vec3<f32>(1.0));
        alpha = clamp(fma(convolve_unscale_24(out_a), inverse, config.rect_x0 * 255.0), 0.0, 255.0);
    } else if (abs(config.rect_x0) > bitcast<f32>(0x7f7fffffu) / 255.0) {
        // Keep a finite bias finite until its addition to the normalized quotient.
        straight = clamp(sum / divisor + vec3<f32>(config.rect_x0), vec3<f32>(0.0), vec3<f32>(1.0));
        alpha = clamp((out_a / 255.0) / divisor + config.rect_x0, 0.0, 1.0) * 255.0;
    } else {
        straight = clamp(fma(sum, vec3<f32>(reciprocal), vec3<f32>(config.rect_x0)), vec3<f32>(0.0), vec3<f32>(1.0));
        alpha = clamp(fma(out_a, reciprocal, config.rect_x0 * 255.0), 0.0, 255.0);
    }
    if (config.kernel_preserve_alpha == 1u) {
        alpha = f32((source_pixel_ix(dst_ix) >> 24u) & 255u);
    }
    target_store_ix(dst_ix, u32(fma(straight.r, alpha, 0.5)) |
        (u32(fma(straight.g, alpha, 0.5)) << 8u) | (u32(fma(straight.b, alpha, 0.5)) << 16u) |
        (u32(alpha + 0.5) << 24u));
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_lighting_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let dst_ix = target_ix_at(xy.x, xy.y);
    var no_light = 0u;
    if (config.lighting_output_kind == 0u) {
        no_light = 0xff000000u;
    }

    let alpha = source_alpha_at(xy.x, xy.y);
    let z = alpha * config.surface_scale;
    let dx = alpha_gradient_x(xy.x, xy.y) * config.surface_scale;
    let dy = alpha_gradient_y(xy.x, xy.y) * config.surface_scale;
    // Explicit dot-product order prevents backend contraction from moving lighting
    // across an 8-bit rounding boundary; the same order applies to each direction.
    let normal_len = sqrt(lighting_dot3(vec3<f32>(dx, dy, 1.0), vec3<f32>(dx, dy, 1.0)));

    let world_x = f32(config.surface_origin_x) + f32(xy.x) + 0.5;
    let world_y = f32(config.surface_origin_y) + f32(xy.y) + 0.5;
    var lx = config.light_p0 - world_x;
    var ly = config.light_p1 - world_y;
    var lz = config.light_p2 - z;
    var attenuation = 1.0;
    let eps = 0.000001;

    if (config.light_kind == 0u) {
        let azimuth = config.light_p0 * 0.017453292;
        let elevation = config.light_p1 * 0.017453292;
        lx = cos(azimuth) * cos(elevation);
        ly = sin(azimuth) * cos(elevation);
        lz = sin(elevation);
    } else {
        let len = sqrt(lighting_dot3(vec3<f32>(lx, ly, lz), vec3<f32>(lx, ly, lz)));
        if (len <= eps) {
            target_store_ix(dst_ix, no_light);
            return;
        }
        lx = lx / len;
        ly = ly / len;
        lz = lz / len;

        if (config.light_kind == 2u) {
            var sx = config.light_p3 - config.light_p0;
            var sy = config.light_p4 - config.light_p1;
            var sz = config.light_p5 - config.light_p2;
            let slen = sqrt(lighting_dot3(vec3<f32>(sx, sy, sz), vec3<f32>(sx, sy, sz)));
            if (slen <= eps) {
                target_store_ix(dst_ix, no_light);
                return;
            }
            sx = sx / slen;
            sy = sy / slen;
            sz = sz / slen;
            let focus = -lighting_dot3(vec3<f32>(lx, ly, lz), vec3<f32>(sx, sy, sz));
            if (focus < 0.0) {
                target_store_ix(dst_ix, no_light);
                return;
            }
            if (config.light_p7 >= 0.0 && focus < cos(config.light_p7 * 0.017453292)) {
                target_store_ix(dst_ix, no_light);
                return;
            }
            attenuation = lighting_power(focus, config.light_p6);
        }
    }

    if (config.lighting_output_kind == 0u) {
        let nx = -dx / normal_len;
        let ny = -dy / normal_len;
        let nz = 1.0 / normal_len;
        let amount = config.light_constant * attenuation * max(lighting_dot3(vec3<f32>(nx, ny, nz), vec3<f32>(lx, ly, lz)), 0.0);
        target_store_ix(dst_ix, pack_premul_rgba8(
            clamp(config.light_r * amount, 0.0, 1.0),
            clamp(config.light_g * amount, 0.0, 1.0),
            clamp(config.light_b * amount, 0.0, 1.0),
            1.0,
        ));
    } else {
        let hx = lx;
        let hy = ly;
        let hz = lz + 1.0;
        let hlen = sqrt(lighting_dot3(vec3<f32>(hx, hy, hz), vec3<f32>(hx, hy, hz)));
        if (hlen <= eps) {
            target_store_ix(dst_ix, no_light);
            return;
        }
        // Normalize the dot once: dividing both vectors component by component
        // adds rounding and lets compilers reassociate the subsequent products.
        // The equivalent scalar expression also avoids six component divisions.
        let normal_dot_half = max(lighting_dot3(
            vec3<f32>(-dx, -dy, 1.0), vec3<f32>(hx, hy, hz),
        ) / (normal_len * hlen), 0.0);
        let amount = config.light_constant * attenuation * lighting_power(normal_dot_half, config.specular_exponent);
        let r = clamp(config.light_r * amount, 0.0, 1.0);
        let g = clamp(config.light_g * amount, 0.0, 1.0);
        let b = clamp(config.light_b * amount, 0.0, 1.0);
        target_store_ix(dst_ix, pack_premul_rgba8(r, g, b, max(max(r, g), b)));
    }
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_liquid_glass_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    let world_x = f32(xy.x) + 0.5;
    let world_y = f32(xy.y) + 0.5;

    let distance = liquid_glass_round_rect_distance(
        world_x,
        world_y,
        config.rect_x0,
        config.rect_y0,
        config.rect_x1,
        config.rect_y1,
        config.radius_top_left,
        config.radius_top_right,
        config.radius_bottom_left,
        config.radius_bottom_right,
    );
    let base = source_pixel_ix(ix);
    let surface_height = f32(max(config.height, 1u));
    let distance_norm = distance / surface_height;
    if (distance_norm >= LIQUID_GLASS_ACTIVE_DISTANCE_NORM) {
        target_store_ix(ix, base);
        return;
    }

    target_store_ix(ix, liquid_glass_pixel(
        base,
        world_x,
        world_y,
        f32(xy.x),
        f32(xy.y),
        distance,
        distance_norm,
        surface_height,
    ));
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_liquid_glass_rect_composite_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }

    let xy = xy_for_region_ix(region_ix);
    let world_x = f32(xy.x) + 0.5;
    let world_y = f32(xy.y) + 0.5;
    let distance = liquid_glass_round_rect_distance(
        world_x,
        world_y,
        config.rect_x0,
        config.rect_y0,
        config.rect_x1,
        config.rect_y1,
        config.radius_top_left,
        config.radius_top_right,
        config.radius_bottom_left,
        config.radius_bottom_right,
    );
    let mask_distance = rect_sdf_distance(
        world_x,
        world_y,
        config.rect_x0,
        config.rect_y0,
        config.rect_x1,
        config.rect_y1,
        config.radius_top_left,
        config.radius_top_right,
        config.radius_bottom_left,
        config.radius_bottom_right,
    );
    let source_alpha = coverage_to_u8(sdf_coverage_from_dist(mask_distance));
    if (source_alpha == 0u) {
        return;
    }

    let ix = target_ix_at(xy.x, xy.y);
    let base = source_pixel_ix(ix);
    let surface_height = f32(max(config.height, 1u));
    let distance_norm = distance / surface_height;
    var source = base;
    if (distance_norm < LIQUID_GLASS_ACTIVE_DISTANCE_NORM) {
        source = liquid_glass_pixel(
            base,
            world_x,
            world_y,
            f32(xy.x),
            f32(xy.y),
            distance,
            distance_norm,
            surface_height,
        );
    }
    source = scale_premul_u8(source, source_alpha);
    target_store_ix(ix, src_over_premul_u8(target_load_ix(ix), source));
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
