fn filter_svg_mask_coverage_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
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

@compute @workgroup_size(256)
fn filter_color_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, apply_color_filter_pixel(target_load_ix(ix), config.filter_kind, config.amount));
}

@compute @workgroup_size(256)
fn filter_color_matrix_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, apply_color_matrix_pixel(target_load_ix(ix)));
}

@compute @workgroup_size(256)
fn filter_component_transfer_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, apply_component_transfer_pixel(target_load_ix(ix), config.table_index));
}

@compute @workgroup_size(256)
fn filter_blend_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, blend_premul_u8(aux_pixel_ix(ix), source_pixel_ix(ix), config.blend_mode));
}

@compute @workgroup_size(256)
fn filter_composite_inputs_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
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

@compute @workgroup_size(256)
fn filter_displacement_map_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    let map = aux_pixel_ix(ix);
    let dx = filter_displacement_channel(map, config.kernel_edge_mode, config.lighting_output_kind) - 0.5;
    let dy = filter_displacement_channel(map, config.kernel_preserve_alpha, config.lighting_output_kind) - 0.5;
    let sx = i32(round(f32(xy.x) + dx * config.amount));
    let sy = i32(round(f32(xy.y) + dy * config.rect_x0));
    var out = 0u;
    if (sx >= 0 && sx < i32(config.width) && sy >= 0 && sy < i32(config.height)) {
        out = source_pixel_at(u32(sx), u32(sy));
    }
    target_store_ix(ix, out);
}

@compute @workgroup_size(256)
fn filter_convolve_matrix_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let dst_ix = target_ix_at(xy.x, xy.y);
    let divisor = config.amount;
    if (config.kernel_columns == 0u || config.kernel_rows == 0u || divisor == 0.0) {
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
            out_r += straight_channel(sample & 255u, alpha) * weight;
            out_g += straight_channel((sample >> 8u) & 255u, alpha) * weight;
            out_b += straight_channel((sample >> 16u) & 255u, alpha) * weight;
            out_a += (f32(alpha) / 255.0) * weight;
            kx += 1u;
        }
        ky += 1u;
    }

    let base_alpha = f32((source_pixel_ix(dst_ix) >> 24u) & 255u) / 255.0;
    var alpha = clamp(out_a / divisor + config.rect_x0, 0.0, 1.0);
    if (config.kernel_preserve_alpha == 1u) {
        alpha = base_alpha;
    }
    let r = clamp(out_r / divisor + config.rect_x0, 0.0, 1.0);
    let g = clamp(out_g / divisor + config.rect_x0, 0.0, 1.0);
    let b = clamp(out_b / divisor + config.rect_x0, 0.0, 1.0);
    target_store_ix(dst_ix, pack_premul_rgba8(r * alpha, g * alpha, b * alpha, alpha));
}

@compute @workgroup_size(256)
fn filter_lighting_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
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
    let normal_len = sqrt(dx * dx + dy * dy + 1.0);
    let nx = -dx / normal_len;
    let ny = -dy / normal_len;
    let nz = 1.0 / normal_len;

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
        let len = sqrt(lx * lx + ly * ly + lz * lz);
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
            let slen = sqrt(sx * sx + sy * sy + sz * sz);
            if (slen <= eps) {
                target_store_ix(dst_ix, no_light);
                return;
            }
            sx = sx / slen;
            sy = sy / slen;
            sz = sz / slen;
            let focus = max(-(lx * sx + ly * sy + lz * sz), 0.0);
            if (config.light_p7 >= 0.0 && focus < cos(config.light_p7 * 0.017453292)) {
                target_store_ix(dst_ix, no_light);
                return;
            }
            attenuation = pow(focus, max(config.light_p6, 0.0));
        }
    }

    if (config.lighting_output_kind == 0u) {
        let amount = config.light_constant * attenuation * max(nx * lx + ny * ly + nz * lz, 0.0);
        target_store_ix(dst_ix, pack_premul_rgba8(
            clamp(config.light_r * amount, 0.0, 1.0),
            clamp(config.light_g * amount, 0.0, 1.0),
            clamp(config.light_b * amount, 0.0, 1.0),
            1.0,
        ));
    } else {
        var hx = lx;
        var hy = ly;
        var hz = lz + 1.0;
        let hlen = sqrt(hx * hx + hy * hy + hz * hz);
        if (hlen <= eps) {
            target_store_ix(dst_ix, no_light);
            return;
        }
        hx = hx / hlen;
        hy = hy / hlen;
        hz = hz / hlen;

        let normal_dot_half = max(nx * hx + ny * hy + nz * hz, 0.0);
        let amount = config.light_constant * attenuation * pow(normal_dot_half, max(config.specular_exponent, 0.0));
        let r = clamp(config.light_r * amount, 0.0, 1.0);
        let g = clamp(config.light_g * amount, 0.0, 1.0);
        let b = clamp(config.light_b * amount, 0.0, 1.0);
        target_store_ix(dst_ix, pack_premul_rgba8(r, g, b, max(max(r, g), b)));
    }
}

@compute @workgroup_size(256)
fn filter_liquid_glass_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
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

@compute @workgroup_size(256)
fn filter_liquid_glass_rect_composite_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
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
    // The liquid glass distance matches the rounded rect mask, so reuse it for coverage.
    let source_alpha = coverage_to_u8(sdf_coverage_from_dist(distance));
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

@compute @workgroup_size(256)
fn filter_liquid_glass_simple_rect_composite_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
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
    let source_alpha = coverage_to_u8(sdf_coverage_from_dist(distance));
    if (source_alpha == 0u) {
        return;
    }

    let ix = target_ix_at(xy.x, xy.y);
    let base = source_pixel_ix(ix);
    let surface_height = f32(max(config.height, 1u));
    let distance_norm = distance / surface_height;
    var source = base;
    if (distance_norm < LIQUID_GLASS_ACTIVE_DISTANCE_NORM) {
        source = liquid_glass_simple_pixel(
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

@compute @workgroup_size(256)
