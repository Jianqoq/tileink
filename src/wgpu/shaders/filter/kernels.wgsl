const SHARED_BLUR_TILE_WIDTH: u32 = 16u;
const SHARED_BLUR_TILE_HEIGHT: u32 = 16u;
const SHARED_BLUR_MAX_RADIUS: u32 = 16u;
const SHARED_BLUR_WORKGROUP_SIZE: u32 = SHARED_BLUR_TILE_WIDTH * SHARED_BLUR_TILE_HEIGHT;
var<workgroup> shared_blur_pixels: array<u32, 768>;

@compute @workgroup_size(256)
fn filter_clear_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    target_store_ix(target_ix_for_region_ix(region_ix), config.clear_color);
}

@compute @workgroup_size(256)
fn filter_copy_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, source_pixel_ix(ix));
}

@compute @workgroup_size(256)
fn filter_source_alpha_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, source_pixel_ix(ix) & 0xff000000u);
}

@compute @workgroup_size(256)
fn filter_source_over_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, blend_premul_u8(target_load_ix(ix), source_pixel_ix(ix), 3u << 8u));
}

@compute @workgroup_size(256)
fn filter_tile_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let source_x0 = u32(config.rect_x0);
    let source_y0 = u32(config.rect_y0);
    let source_width = u32(config.rect_x1) - source_x0;
    let source_height = u32(config.rect_y1) - source_y0;
    if (source_width == 0u || source_height == 0u) {
        return;
    }
    let sx = source_x0 + (xy.x + source_width - (source_x0 % source_width)) % source_width;
    let sy = source_y0 + (xy.y + source_height - (source_y0 % source_height)) % source_height;
    target_store_at(xy.x, xy.y, source_pixel_at(sx, sy));
}

@compute @workgroup_size(256)
fn filter_offset_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let sx = i32(xy.x) - config.offset_x;
    let sy = i32(xy.y) - config.offset_y;
    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    var pixel = 0u;
    if (
        sx >= i32(config.region_x0) &&
        sx < region_x1 &&
        sy >= i32(config.region_y0) &&
        sy < region_y1
    ) {
        pixel = source_pixel_at(u32(sx), u32(sy));
    }
    target_store_at(xy.x, xy.y, pixel);
}

@compute @workgroup_size(256)
fn filter_turbulence_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    target_store_at(xy.x, xy.y, filter_turbulence_pixel(f32(xy.x), f32(xy.y)));
}

@compute @workgroup_size(256)
fn filter_flood_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    target_store_at(xy.x, xy.y, sample_brush(config.brush_offset, f32(xy.x) + 0.5, f32(xy.y) + 0.5));
}

@compute @workgroup_size(256)
fn filter_drop_shadow_mask_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }

    let xy = xy_for_region_ix(region_ix);
    let alpha = source_pixel_at(xy.x, xy.y) >> 24u;
    if (alpha == 0u) {
        return;
    }

    let tx = i32(xy.x) + config.offset_x;
    let ty = i32(xy.y) + config.offset_y;
    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    if (
        tx >= i32(config.region_x0) &&
        tx < region_x1 &&
        ty >= i32(config.region_y0) &&
        ty < region_y1
    ) {
        target_store_at(u32(tx), u32(ty), gray_alpha(alpha));
    }
}

@compute @workgroup_size(256)
fn filter_morphology_axis_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let ix = target_ix_at(xy.x, xy.y);
    let radius = config.morphology_radius;
    let morph_operator = config.morphology_operator;
    let axis = config.morphology_axis;
    let pos = select(xy.y, xy.x, axis == 0u);
    let line_len = select(config.height, config.width, axis == 0u);

    if (morph_operator == 0u && (pos < radius || pos + radius >= line_len)) {
        target_store_ix(ix, 0u);
        return;
    }

    var out_r = 1.0;
    var out_g = 1.0;
    var out_b = 1.0;
    var out_a = 1.0;
    if (morph_operator == 1u) {
        out_r = 0.0;
        out_g = 0.0;
        out_b = 0.0;
        out_a = 0.0;
    }

    var start = 0u;
    if (pos > radius) {
        start = pos - radius;
    }
    var end = line_len - 1u;
    if (pos + radius < end) {
        end = pos + radius;
    }

    var sample_pos = start;
    loop {
        if (sample_pos > end) {
            break;
        }
        let sx = select(xy.x, sample_pos, axis == 0u);
        let sy = select(sample_pos, xy.y, axis == 0u);
        let sample = source_pixel_at(sx, sy);
        let alpha = (sample >> 24u) & 255u;
        let sample_r = straight_channel(sample & 255u, alpha);
        let sample_g = straight_channel((sample >> 8u) & 255u, alpha);
        let sample_b = straight_channel((sample >> 16u) & 255u, alpha);
        let sample_a = f32(alpha) / 255.0;

        if (morph_operator == 1u) {
            out_r = max(out_r, sample_r);
            out_g = max(out_g, sample_g);
            out_b = max(out_b, sample_b);
            out_a = max(out_a, sample_a);
        } else {
            out_r = min(out_r, sample_r);
            out_g = min(out_g, sample_g);
            out_b = min(out_b, sample_b);
            out_a = min(out_a, sample_a);
        }
        sample_pos += 1u;
    }

    target_store_ix(ix, pack_premul_rgba8(out_r * out_a, out_g * out_a, out_b * out_a, out_a));
}

@compute @workgroup_size(256)
fn filter_downsample_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }

    let factor = max(config.downsample, 1u);
    let xy = xy_for_region_ix(region_ix);
    let source_x0 = u32(config.rect_x0);
    let source_y0 = u32(config.rect_y0);
    let source_x1 = u32(config.rect_x1);
    let source_y1 = u32(config.rect_y1);
    let cell_x0 = max(xy.x * factor, source_x0);
    let cell_y0 = max(xy.y * factor, source_y0);
    let cell_x1 = min((xy.x + 1u) * factor, source_x1);
    let cell_y1 = min((xy.y + 1u) * factor, source_y1);
    if (cell_x0 >= cell_x1 || cell_y0 >= cell_y1) {
        target_store_at(xy.x, xy.y, 0u);
        return;
    }

    if (config.downsample_filter == 0u) {
        let sx = clamp((cell_x0 + cell_x1 - 1u) / 2u, source_x0, source_x1 - 1u);
        let sy = clamp((cell_y0 + cell_y1 - 1u) / 2u, source_y0, source_y1 - 1u);
        target_store_at(xy.x, xy.y, source_pixel_at(sx, sy));
        return;
    }

    var acc = vec4<f32>(0.0);
    var count = 0.0;
    var sy = cell_y0;
    loop {
        if (sy >= cell_y1) {
            break;
        }
        var sx = cell_x0;
        loop {
            if (sx >= cell_x1) {
                break;
            }
            acc += rgba8_to_unorm(source_pixel_at(sx, sy));
            count += 1.0;
            sx += 1u;
        }
        sy += 1u;
    }

    let avg = acc / count;
    target_store_at(xy.x, xy.y, pack_premul_rgba8(avg.r, avg.g, avg.b, avg.a));
}

fn upsampled_source_pixel_at(
    xy: vec2<u32>,
    source_x0: u32,
    source_y0: u32,
    source_x1: u32,
    source_y1: u32,
) -> u32 {
    let factor = f32(max(config.downsample, 1u));
    let max_x = f32(source_x1 - 1u);
    let max_y = f32(source_y1 - 1u);
    let sample_x = clamp((f32(xy.x) + 0.5) / factor - 0.5, f32(source_x0), max_x);
    let sample_y = clamp((f32(xy.y) + 0.5) / factor - 0.5, f32(source_y0), max_y);
    let x0 = u32(floor(sample_x));
    let y0 = u32(floor(sample_y));
    let x1 = min(x0 + 1u, source_x1 - 1u);
    let y1 = min(y0 + 1u, source_y1 - 1u);
    let tx = sample_x - floor(sample_x);
    let ty = sample_y - floor(sample_y);
    if (config.upsample_filter == 0u) {
        return source_pixel_at(u32(round(sample_x)), u32(round(sample_y)));
    }
    let top = lerp_premul_u8(source_pixel_at(x0, y0), source_pixel_at(x1, y0), tx);
    let bottom = lerp_premul_u8(source_pixel_at(x0, y1), source_pixel_at(x1, y1), tx);
    return lerp_premul_u8(top, bottom, ty);
}

@compute @workgroup_size(256)
fn filter_upsample_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }

    let source_x0 = u32(config.rect_x0);
    let source_y0 = u32(config.rect_y0);
    let source_x1 = u32(config.rect_x1);
    let source_y1 = u32(config.rect_y1);
    if (source_x0 >= source_x1 || source_y0 >= source_y1) {
        return;
    }

    let xy = xy_for_region_ix(region_ix);
    target_store_at(xy.x, xy.y, upsampled_source_pixel_at(xy, source_x0, source_y0, source_x1, source_y1));
}

@compute @workgroup_size(256)
fn filter_upsample_rect_composite_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }

    let source_x0 = config.source_x0;
    let source_y0 = config.source_y0;
    let source_x1 = config.source_x1;
    let source_y1 = config.source_y1;
    if (source_x0 >= source_x1 || source_y0 >= source_y1) {
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
    let source = scale_premul_u8(
        upsampled_source_pixel_at(xy, source_x0, source_y0, source_x1, source_y1),
        source_alpha,
    );
    target_store_ix(ix, src_over_premul_u8(target_load_ix(ix), source));
}

fn filter_blur_half_width(std_dev: f32) -> i32 {
    return i32(max(ceil(std_dev * 3.0), 1.0));
}

fn filter_blur_pixel_global(xy: vec2<u32>, dst_ix: u32, std_dev: f32) -> u32 {
    let half_width = i32(max(ceil(std_dev * 3.0), 1.0));
    let sigma = max(std_dev, 0.0001);
    let two_sigma_sq = 2.0 * sigma * sigma;
    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    let base_x = i32(xy.x);
    let base_y = i32(xy.y);

    let center = source_pixel_ix(dst_ix);
    var sum = 1.0;
    var r = f32(center & 255u);
    var g = f32((center >> 8u) & 255u);
    var b = f32((center >> 16u) & 255u);
    var a = f32((center >> 24u) & 255u);

    var weight = exp(-1.0 / two_sigma_sq);
    let weight_ratio_decay = exp(-2.0 / two_sigma_sq);
    var weight_ratio = weight * weight_ratio_decay;
    var d = 1i;
    loop {
        if (d > half_width) {
            break;
        }
        sum += 2.0 * weight;
        var sample_x = base_x + d;
        var sample_y = base_y;
        if (config.blur_axis == 0u) {
            sample_y = base_y;
        } else {
            sample_x = base_x;
            sample_y = base_y + d;
        }
        if (
            sample_x >= i32(config.region_x0) &&
            sample_x < region_x1 &&
            sample_y >= i32(config.region_y0) &&
            sample_y < region_y1
        ) {
            let px = source_pixel_at(u32(sample_x), u32(sample_y));
            r += f32(px & 255u) * weight;
            g += f32((px >> 8u) & 255u) * weight;
            b += f32((px >> 16u) & 255u) * weight;
            a += f32((px >> 24u) & 255u) * weight;
        }

        sample_x = base_x - d;
        sample_y = base_y;
        if (config.blur_axis != 0u) {
            sample_x = base_x;
            sample_y = base_y - d;
        }
        if (
            sample_x >= i32(config.region_x0) &&
            sample_x < region_x1 &&
            sample_y >= i32(config.region_y0) &&
            sample_y < region_y1
        ) {
            let px = source_pixel_at(u32(sample_x), u32(sample_y));
            r += f32(px & 255u) * weight;
            g += f32((px >> 8u) & 255u) * weight;
            b += f32((px >> 16u) & 255u) * weight;
            a += f32((px >> 24u) & 255u) * weight;
        }

        weight *= weight_ratio;
        weight_ratio *= weight_ratio_decay;
        d += 1i;
    }

    var scale = 0.0;
    if (sum > 0.0) {
        scale = 1.0 / (255.0 * sum);
    }
    return pack_premul_rgba8(r * scale, g * scale, b * scale, a * scale);
}

fn filter_blur_pixel_shared(local_xy: vec2<u32>, half_width: i32, std_dev: f32) -> u32 {
    let radius = u32(half_width);
    let sigma = max(std_dev, 0.0001);
    let two_sigma_sq = 2.0 * sigma * sigma;

    var center_ix = (local_xy.y + radius) * SHARED_BLUR_TILE_WIDTH + local_xy.x;
    if (config.blur_axis == 0u) {
        let stride = SHARED_BLUR_TILE_WIDTH + 2u * radius;
        center_ix = local_xy.y * stride + radius + local_xy.x;
    }

    let center = shared_blur_pixels[center_ix];
    var sum = 1.0;
    var r = f32(center & 255u);
    var g = f32((center >> 8u) & 255u);
    var b = f32((center >> 16u) & 255u);
    var a = f32((center >> 24u) & 255u);

    var weight = exp(-1.0 / two_sigma_sq);
    let weight_ratio_decay = exp(-2.0 / two_sigma_sq);
    var weight_ratio = weight * weight_ratio_decay;
    var d = 1i;
    loop {
        if (d > half_width) {
            break;
        }
        sum += 2.0 * weight;

        var plus_ix = (local_xy.y + radius + u32(d)) * SHARED_BLUR_TILE_WIDTH + local_xy.x;
        var minus_ix = (local_xy.y + radius - u32(d)) * SHARED_BLUR_TILE_WIDTH + local_xy.x;
        if (config.blur_axis == 0u) {
            let stride = SHARED_BLUR_TILE_WIDTH + 2u * radius;
            plus_ix = local_xy.y * stride + radius + local_xy.x + u32(d);
            minus_ix = local_xy.y * stride + radius + local_xy.x - u32(d);
        }

        let plus = shared_blur_pixels[plus_ix];
        r += f32(plus & 255u) * weight;
        g += f32((plus >> 8u) & 255u) * weight;
        b += f32((plus >> 16u) & 255u) * weight;
        a += f32((plus >> 24u) & 255u) * weight;

        let minus = shared_blur_pixels[minus_ix];
        r += f32(minus & 255u) * weight;
        g += f32((minus >> 8u) & 255u) * weight;
        b += f32((minus >> 16u) & 255u) * weight;
        a += f32((minus >> 24u) & 255u) * weight;

        weight *= weight_ratio;
        weight_ratio *= weight_ratio_decay;
        d += 1i;
    }

    var scale = 0.0;
    if (sum > 0.0) {
        scale = 1.0 / (255.0 * sum);
    }
    return pack_premul_rgba8(r * scale, g * scale, b * scale, a * scale);
}

@compute @workgroup_size(256)
fn filter_blur_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }

    let std_dev = max(config.amount, 0.0);
    let xy = xy_for_region_ix(region_ix);
    let dst_ix = target_ix_at(xy.x, xy.y);
    if (std_dev <= 0.0) {
        target_store_ix(dst_ix, source_pixel_ix(dst_ix));
        return;
    }

    target_store_ix(dst_ix, filter_blur_pixel_global(xy, dst_ix, std_dev));
}

@compute @workgroup_size(16, 16)
fn filter_blur_shared_region(
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
) {
    let std_dev = max(config.amount, 0.0);
    let tile_x0 = config.region_x0 + workgroup_id.x * SHARED_BLUR_TILE_WIDTH;
    let tile_y0 = config.region_y0 + workgroup_id.y * SHARED_BLUR_TILE_HEIGHT;
    let xy = vec2<u32>(tile_x0 + local_id.x, tile_y0 + local_id.y);
    let in_region = xy.x < config.region_x0 + config.region_width &&
        xy.y < config.region_y0 + config.region_height;

    if (std_dev <= 0.0) {
        if (in_region) {
            let ix = target_ix_at(xy.x, xy.y);
            target_store_ix(ix, source_pixel_ix(ix));
        }
        return;
    }

    let half_width = filter_blur_half_width(std_dev);
    if (half_width > i32(SHARED_BLUR_MAX_RADIUS)) {
        if (in_region) {
            let ix = target_ix_at(xy.x, xy.y);
            target_store_ix(ix, filter_blur_pixel_global(xy, ix, std_dev));
        }
        return;
    }

    let radius = u32(half_width);
    let local_ix = local_id.y * SHARED_BLUR_TILE_WIDTH + local_id.x;
    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    var sample_count = SHARED_BLUR_TILE_WIDTH * (SHARED_BLUR_TILE_HEIGHT + 2u * radius);
    if (config.blur_axis == 0u) {
        sample_count = (SHARED_BLUR_TILE_WIDTH + 2u * radius) * SHARED_BLUR_TILE_HEIGHT;
    }

    var load_ix = local_ix;
    loop {
        if (load_ix >= sample_count) {
            break;
        }

        var sx = 0i;
        var sy = 0i;
        if (config.blur_axis == 0u) {
            let stride = SHARED_BLUR_TILE_WIDTH + 2u * radius;
            sx = i32(tile_x0 + load_ix % stride) - i32(radius);
            sy = i32(tile_y0 + load_ix / stride);
        } else {
            sx = i32(tile_x0 + load_ix % SHARED_BLUR_TILE_WIDTH);
            sy = i32(tile_y0 + load_ix / SHARED_BLUR_TILE_WIDTH) - i32(radius);
        }

        var pixel = 0u;
        if (
            sx >= i32(config.region_x0) &&
            sx < region_x1 &&
            sy >= i32(config.region_y0) &&
            sy < region_y1
        ) {
            pixel = source_pixel_at(u32(sx), u32(sy));
        }
        shared_blur_pixels[load_ix] = pixel;
        load_ix += SHARED_BLUR_WORKGROUP_SIZE;
    }

    workgroupBarrier();

    if (!in_region) {
        return;
    }
    target_store_at(
        xy.x,
        xy.y,
        filter_blur_pixel_shared(vec2<u32>(local_id.x, local_id.y), half_width, std_dev),
    );
}

@compute @workgroup_size(256)
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

@compute @workgroup_size(256)
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

