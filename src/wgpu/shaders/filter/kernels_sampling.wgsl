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
    if (config.upsample_filter == 0u) {
        return source_pixel_at(u32(round(sample_x)), u32(round(sample_y)));
    }
    let sample = filter_source_sample_premul(sample_x, sample_y);
    return pack_premul_rgba8(sample.r, sample.g, sample.b, sample.a);
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

fn filter_blur_source_sample_pixel(x: f32, y: f32) -> vec4<f32> {
    return filter_source_sample_premul(x, y) * 255.0;
}

fn filter_blur_source_pixel(x: u32, y: u32) -> vec4<f32> {
    let px = source_pixel_at(x, y);
    return vec4<f32>(
        f32(px & 255u),
        f32((px >> 8u) & 255u),
        f32((px >> 16u) & 255u),
        f32((px >> 24u) & 255u),
    );
}

fn filter_blur_pair_in_region(
    base_x: i32,
    base_y: i32,
    offset: f32,
    region_x1: i32,
    region_y1: i32,
) -> bool {
    var x0 = f32(base_x) + offset;
    var x1 = f32(base_x) + offset + 1.0;
    var y0 = f32(base_y);
    var y1 = f32(base_y);
    if (config.blur_axis != 0u) {
        x0 = f32(base_x);
        x1 = f32(base_x);
        y0 = f32(base_y) + offset;
        y1 = f32(base_y) + offset + 1.0;
    }
    return x0 >= f32(config.region_x0) &&
        x1 < f32(region_x1) &&
        y0 >= f32(config.region_y0) &&
        y1 < f32(region_y1);
}

fn filter_blur_sample_pair(base_x: i32, base_y: i32, offset: f32) -> vec4<f32> {
    if (config.blur_axis == 0u) {
        return filter_blur_source_sample_pixel(f32(base_x) + offset, f32(base_y));
    }
    return filter_blur_source_sample_pixel(f32(base_x), f32(base_y) + offset);
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
        let next_d = d + 1i;
        let has_pair = next_d <= half_width;
        let next_weight = weight * weight_ratio;
        let pair_weight = weight + select(0.0, next_weight, has_pair);
        sum += 2.0 * pair_weight;

        var sample_x = base_x + d;
        var sample_y = base_y;
        if (config.blur_axis == 0u) {
            sample_y = base_y;
        } else {
            sample_x = base_x;
            sample_y = base_y + d;
        }
        if (has_pair && filter_blur_pair_in_region(base_x, base_y, f32(d), region_x1, region_y1)) {
            let offset = f32(d) + next_weight / pair_weight;
            let sample = filter_blur_sample_pair(base_x, base_y, offset);
            r += sample.r * pair_weight;
            g += sample.g * pair_weight;
            b += sample.b * pair_weight;
            a += sample.a * pair_weight;
        } else {
            if (
                sample_x >= i32(config.region_x0) &&
                sample_x < region_x1 &&
                sample_y >= i32(config.region_y0) &&
                sample_y < region_y1
            ) {
                let sample = filter_blur_source_pixel(u32(sample_x), u32(sample_y));
                r += sample.r * weight;
                g += sample.g * weight;
                b += sample.b * weight;
                a += sample.a * weight;
            }
            if (has_pair) {
                sample_x = base_x + next_d;
                sample_y = base_y;
                if (config.blur_axis != 0u) {
                    sample_x = base_x;
                    sample_y = base_y + next_d;
                }
                if (
                    sample_x >= i32(config.region_x0) &&
                    sample_x < region_x1 &&
                    sample_y >= i32(config.region_y0) &&
                    sample_y < region_y1
                ) {
                    let sample = filter_blur_source_pixel(u32(sample_x), u32(sample_y));
                    r += sample.r * next_weight;
                    g += sample.g * next_weight;
                    b += sample.b * next_weight;
                    a += sample.a * next_weight;
                }
            }
        }

        sample_x = base_x - d;
        sample_y = base_y;
        if (config.blur_axis != 0u) {
            sample_x = base_x;
            sample_y = base_y - d;
        }
        if (has_pair && filter_blur_pair_in_region(base_x, base_y, -f32(next_d), region_x1, region_y1)) {
            let offset = -(f32(d) + next_weight / pair_weight);
            let sample = filter_blur_sample_pair(base_x, base_y, offset);
            r += sample.r * pair_weight;
            g += sample.g * pair_weight;
            b += sample.b * pair_weight;
            a += sample.a * pair_weight;
        } else {
            if (
                sample_x >= i32(config.region_x0) &&
                sample_x < region_x1 &&
                sample_y >= i32(config.region_y0) &&
                sample_y < region_y1
            ) {
                let sample = filter_blur_source_pixel(u32(sample_x), u32(sample_y));
                r += sample.r * weight;
                g += sample.g * weight;
                b += sample.b * weight;
                a += sample.a * weight;
            }
            if (has_pair) {
                sample_x = base_x - next_d;
                sample_y = base_y;
                if (config.blur_axis != 0u) {
                    sample_x = base_x;
                    sample_y = base_y - next_d;
                }
                if (
                    sample_x >= i32(config.region_x0) &&
                    sample_x < region_x1 &&
                    sample_y >= i32(config.region_y0) &&
                    sample_y < region_y1
                ) {
                    let sample = filter_blur_source_pixel(u32(sample_x), u32(sample_y));
                    r += sample.r * next_weight;
                    g += sample.g * next_weight;
                    b += sample.b * next_weight;
                    a += sample.a * next_weight;
                }
            }
        }

        if (has_pair) {
            weight = next_weight * (weight_ratio * weight_ratio_decay);
            weight_ratio *= weight_ratio_decay * weight_ratio_decay;
            d += 2i;
        } else {
            weight = next_weight;
            weight_ratio *= weight_ratio_decay;
            d += 1i;
        }
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
