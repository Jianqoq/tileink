fn filter_composite_drop_shadow_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }

    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    let alpha = aux_pixel_ix(ix) >> 24u;
    let shadow_color = sample_brush(config.brush_offset, f32(xy.x) + 0.5, f32(xy.y) + 0.5);
    let shadow = scale_premul_u8(shadow_color, alpha);
    target_store_ix(ix, src_over_premul_u8(shadow, target_load_ix(ix)));
}

@compute @workgroup_size(256)
fn filter_layer_mask_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let alpha = layer_stack_alpha_at(config.draw_ix, xy.x, xy.y);
    target_store_at(xy.x, xy.y, gray_alpha(alpha));
}

@compute @workgroup_size(256)
fn filter_rect_mask_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let dist = rect_sdf_distance(
        f32(xy.x) + 0.5,
        f32(xy.y) + 0.5,
        config.rect_x0,
        config.rect_y0,
        config.rect_x1,
        config.rect_y1,
        config.radius_top_left,
        config.radius_top_right,
        config.radius_bottom_left,
        config.radius_bottom_right,
    );
    target_store_at(xy.x, xy.y, gray_alpha(coverage_to_u8(sdf_coverage_from_dist(dist))));
}

@compute @workgroup_size(256)
fn filter_path_mask_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let px = f32(xy.x) + 0.5;
    let py = f32(xy.y) + 0.5;
    let path_index = config.table_index;
    var winding = 0i;
    if (path_index < arrayLength(&path_range_starts)) {
        var line_ix = path_range_starts[path_index];
        let line_end = path_range_ends[path_index];
        loop {
            if (line_ix >= line_end) {
                break;
            }
            let inv_scale = 0.00390625;
            let y0 = f32(path_p0y[line_ix]) * inv_scale;
            let y1 = f32(path_p1y[line_ix]) * inv_scale;
            var winding_delta = 0i;
            if (y0 <= py) {
                if (y1 > py) {
                    winding_delta = 1i;
                }
            }
            if (y1 <= py) {
                if (y0 > py) {
                    winding_delta = -1i;
                }
            }
            if (winding_delta != 0i) {
                let x0 = f32(path_p0x[line_ix]) * inv_scale;
                let x1 = f32(path_p1x[line_ix]) * inv_scale;
                let t = (py - y0) / (y1 - y0);
                let x_cross = x0 + (x1 - x0) * t;
                if (x_cross > px) {
                    winding += winding_delta;
                }
            }
            line_ix += 1u;
        }
    }

    var alpha = 0u;
    if (winding != 0i) {
        alpha = 255u;
    }
    target_store_at(xy.x, xy.y, gray_alpha(alpha));
}

@compute @workgroup_size(256)
fn filter_composite_direct_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    var source_alpha = 255u;
    if (config.mask_enabled != 0u) {
        source_alpha = combine_alpha(source_alpha, aux_pixel_ix(ix) >> 24u);
    }
    let source = scale_premul_u8(source_pixel_ix(ix), source_alpha);
    target_store_ix(ix, src_over_premul_u8(target_load_ix(ix), source));
}

@compute @workgroup_size(256)
fn filter_composite_rect_direct_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let dist = rect_sdf_distance(
        f32(xy.x) + 0.5,
        f32(xy.y) + 0.5,
        config.rect_x0,
        config.rect_y0,
        config.rect_x1,
        config.rect_y1,
        config.radius_top_left,
        config.radius_top_right,
        config.radius_bottom_left,
        config.radius_bottom_right,
    );
    let source_alpha = coverage_to_u8(sdf_coverage_from_dist(dist));
    if (source_alpha == 0u) {
        return;
    }
    let ix = target_ix_at(xy.x, xy.y);
    let source = scale_premul_u8(source_pixel_ix(ix), source_alpha);
    target_store_ix(ix, src_over_premul_u8(target_load_ix(ix), source));
}

@compute @workgroup_size(256)
fn filter_composite_stack_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    target_store_ix(ix, composite_with_stack(target_load_ix(ix), source_pixel_ix(ix), aux_pixel_ix(ix), xy.x, xy.y, false));
}

@compute @workgroup_size(256)
fn filter_composite_blend_stack_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    target_store_ix(ix, composite_with_stack(target_load_ix(ix), source_pixel_ix(ix), aux_pixel_ix(ix), xy.x, xy.y, true));
}

@compute @workgroup_size(256)
fn filter_composite_surface_direct_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let sx = i32(xy.x) - config.offset_x;
    let sy = i32(xy.y) - config.offset_y;
    if (
        sx < 0 ||
        sy < 0 ||
        sx >= i32(config.kernel_columns) ||
        sy >= i32(config.kernel_rows)
    ) {
        return;
    }

    let ix = target_ix_at(xy.x, xy.y);
    let source = source_pixel_at(u32(sx), u32(sy));
    target_store_ix(ix, src_over_premul_u8(target_load_ix(ix), source));
}

@compute @workgroup_size(256)
fn filter_composite_surface_stack_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let sx = i32(xy.x) - config.offset_x;
    let sy = i32(xy.y) - config.offset_y;
    if (
        sx < 0 ||
        sy < 0 ||
        sx >= i32(config.kernel_columns) ||
        sy >= i32(config.kernel_rows)
    ) {
        return;
    }

    let ix = target_ix_at(xy.x, xy.y);
    target_store_ix(ix, composite_surface_with_stack(target_load_ix(ix), source_pixel_at(u32(sx), u32(sy)), xy.x, xy.y));
}

