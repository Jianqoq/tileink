fn filter_morphology_axis_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
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

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_downsample_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
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

    // Keep the average in stored channel units and quantize once, as in HLSL.
    var acc = vec4<f32>(0.0);
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
            let pixel = source_pixel_at(sx, sy);
            acc += vec4<f32>(f32(pixel & 255u), f32((pixel >> 8u) & 255u),
                f32((pixel >> 16u) & 255u), f32(pixel >> 24u));
            sx += 1u;
        }
        sy += 1u;
    }

    let count = f32((cell_x1 - cell_x0) * (cell_y1 - cell_y0));
    let avg = vec4<u32>(clamp(fma(acc, vec4<f32>(1.0 / count), vec4<f32>(0.5)),
        vec4<f32>(0.0), vec4<f32>(255.0)));
    target_store_at(xy.x, xy.y, avg.r | (avg.g << 8u) | (avg.b << 16u) | (avg.a << 24u));
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

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_upsample_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
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

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_upsample_rect_composite_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
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

fn filter_blur_pack_average(accumulator: vec4<f32>, sum: f32) -> u32 {
    if (sum > 0.0) {
        // Average and quantize in stored channel units. Dividing by 255 and
        // multiplying back permits reciprocal reassociation at byte boundaries.
        let average = fma(accumulator, vec4<f32>(1.0 / sum), vec4<f32>(0.0));
        let channels = vec4<u32>(clamp(
            average + vec4<f32>(0.5), vec4<f32>(0.0), vec4<f32>(255.0),
        ));
        return channels.r | (channels.g << 8u) | (channels.b << 16u) | (channels.a << 24u);
    }
    return 0u;
}

fn filter_blur_source_sample_pixel(x: f32, y: f32) -> vec4<f32> {
    // Materialize the sample in stored channel units before weighting it. This
    // prevents the sampler's UNORM scale from being regrouped with the tap weight.
    return fma(filter_source_sample_premul(x, y), vec4<f32>(255.0), vec4<f32>(0.0));
}

fn filter_blur_source_pixel(x: u32, y: u32) -> vec4<f32> {
    return filter_blur_channels(source_pixel_at(x, y));
}

fn filter_blur_channels(px: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(px & 255u),
        f32((px >> 8u) & 255u),
        f32((px >> 16u) & 255u),
        f32((px >> 24u) & 255u),
    );
}

fn filter_blur_sample_x0() -> u32 {
    return select(config.region_x0, config.source_x0, config.source_x1 > config.source_x0);
}

fn filter_blur_sample_y0() -> u32 {
    return select(config.region_y0, config.source_y0, config.source_y1 > config.source_y0);
}

fn filter_blur_sample_x1() -> u32 {
    return select(
        config.region_x0 + config.region_width,
        config.source_x1,
        config.source_x1 > config.source_x0,
    );
}

fn filter_blur_sample_y1() -> u32 {
    return select(
        config.region_y0 + config.region_height,
        config.source_y1,
        config.source_y1 > config.source_y0,
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
    return x0 >= f32(filter_blur_sample_x0()) &&
        x1 < f32(region_x1) &&
        y0 >= f32(filter_blur_sample_y0()) &&
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
    let region_x0 = i32(filter_blur_sample_x0());
    let region_y0 = i32(filter_blur_sample_y0());
    let region_x1 = i32(filter_blur_sample_x1());
    let region_y1 = i32(filter_blur_sample_y1());
    let base_x = i32(xy.x);
    let base_y = i32(xy.y);

    // Root fix: the center must obey the same source domain as every other tap.
    // Otherwise global blur leaks pixels that shared blur correctly treats as transparent.
    var center = 0u;
    if (base_x >= region_x0 && base_x < region_x1 && base_y >= region_y0 && base_y < region_y1) {
        center = source_pixel_ix(dst_ix);
    }
    var sum = 1.0;
    var accumulator = filter_blur_channels(center);

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
            accumulator = fma(sample, vec4<f32>(pair_weight), accumulator);
        } else {
            if (
                sample_x >= region_x0 &&
                sample_x < region_x1 &&
                sample_y >= region_y0 &&
                sample_y < region_y1
            ) {
                let sample = filter_blur_source_pixel(u32(sample_x), u32(sample_y));
                accumulator = fma(sample, vec4<f32>(weight), accumulator);
            }
            if (has_pair) {
                sample_x = base_x + next_d;
                sample_y = base_y;
                if (config.blur_axis != 0u) {
                    sample_x = base_x;
                    sample_y = base_y + next_d;
                }
                if (
                    sample_x >= region_x0 &&
                    sample_x < region_x1 &&
                    sample_y >= region_y0 &&
                    sample_y < region_y1
                ) {
                    let sample = filter_blur_source_pixel(u32(sample_x), u32(sample_y));
                    accumulator = fma(sample, vec4<f32>(next_weight), accumulator);
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
            accumulator = fma(sample, vec4<f32>(pair_weight), accumulator);
        } else {
            if (
                sample_x >= region_x0 &&
                sample_x < region_x1 &&
                sample_y >= region_y0 &&
                sample_y < region_y1
            ) {
                let sample = filter_blur_source_pixel(u32(sample_x), u32(sample_y));
                accumulator = fma(sample, vec4<f32>(weight), accumulator);
            }
            if (has_pair) {
                sample_x = base_x - next_d;
                sample_y = base_y;
                if (config.blur_axis != 0u) {
                    sample_x = base_x;
                    sample_y = base_y - next_d;
                }
                if (
                    sample_x >= region_x0 &&
                    sample_x < region_x1 &&
                    sample_y >= region_y0 &&
                    sample_y < region_y1
                ) {
                    let sample = filter_blur_source_pixel(u32(sample_x), u32(sample_y));
                    accumulator = fma(sample, vec4<f32>(next_weight), accumulator);
                }
            }
        }

        if (has_pair) {
            // Preserve each product's rounding boundary. Reassociating the
            // three factors makes the recurrence drift across shader targets.
            weight = fma(next_weight, fma(weight_ratio, weight_ratio_decay, 0.0), 0.0);
            weight_ratio = fma(weight_ratio, fma(weight_ratio_decay, weight_ratio_decay, 0.0), 0.0);
            d += 2i;
        } else {
            weight = next_weight;
            weight_ratio *= weight_ratio_decay;
            d += 1i;
        }
    }

    return filter_blur_pack_average(accumulator, sum);
}

// Interior pixels share the same pair-sampling branch for every tap. Check the
// complete support once, avoiding repeated edge branches inside the hot loop.
fn filter_blur_pixel_interior(xy: vec2<u32>, dst_ix: u32, std_dev: f32) -> u32 {
    let half_width = filter_blur_half_width(std_dev);
    let sigma = max(std_dev, 0.0001);
    let two_sigma_sq = 2.0 * sigma * sigma;
    var weight = exp(-1.0 / two_sigma_sq);
    let weight_ratio_decay = exp(-2.0 / two_sigma_sq);
    var weight_ratio = weight * weight_ratio_decay;
    var sum = 1.0;
    let base_x = i32(xy.x);
    let base_y = i32(xy.y);
    var accumulator = filter_blur_channels(source_pixel_ix(dst_ix));
    var d = 1i;
    loop {
        if (d + 1i > half_width) { break; }
        let next_weight = weight * weight_ratio;
        let pair_weight = weight + next_weight;
        sum += 2.0 * pair_weight;
        let offset = f32(d) + next_weight / pair_weight;
        let plus = filter_blur_sample_pair(base_x, base_y, offset);
        accumulator = fma(plus, vec4<f32>(pair_weight), accumulator);
        let minus = filter_blur_sample_pair(base_x, base_y, -offset);
        accumulator = fma(minus, vec4<f32>(pair_weight), accumulator);
        weight = fma(next_weight, fma(weight_ratio, weight_ratio_decay, 0.0), 0.0);
        weight_ratio = fma(weight_ratio, fma(weight_ratio_decay, weight_ratio_decay, 0.0), 0.0);
        d += 2i;
    }
    if (d <= half_width) {
        sum += 2.0 * weight;
        let offset = select(vec2<i32>(0i, d), vec2<i32>(d, 0i), config.blur_axis == 0u);
        let plus_xy = vec2<u32>(vec2<i32>(xy) + offset);
        let minus_xy = vec2<u32>(vec2<i32>(xy) - offset);
        let plus = filter_blur_source_pixel(plus_xy.x, plus_xy.y);
        accumulator = fma(plus, vec4<f32>(weight), accumulator);
        let minus = filter_blur_source_pixel(minus_xy.x, minus_xy.y);
        accumulator = fma(minus, vec4<f32>(weight), accumulator);
    }
    return filter_blur_pack_average(accumulator, sum);
}

fn filter_blur_pixel(xy: vec2<u32>, dst_ix: u32, std_dev: f32) -> u32 {
    let half_width = filter_blur_half_width(std_dev);
    let radius = select(vec2<i32>(0i, half_width), vec2<i32>(half_width, 0i), config.blur_axis == 0u);
    let lower = vec2<i32>(xy) - radius;
    let upper = vec2<i32>(xy) + radius;
    let region_min = vec2<i32>(i32(filter_blur_sample_x0()), i32(filter_blur_sample_y0()));
    let region_max = vec2<i32>(i32(filter_blur_sample_x1()), i32(filter_blur_sample_y1()));
    if (all(lower >= region_min) && all(upper < region_max)) {
        return filter_blur_pixel_interior(xy, dst_ix, std_dev);
    }
    return filter_blur_pixel_global(xy, dst_ix, std_dev);
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
    var accumulator = filter_blur_channels(center);

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

        // Give each weighted tap one explicit accumulation rounding boundary.
        // Implicit contraction can choose different byte-boundary results by API.
        let plus = filter_blur_channels(shared_blur_pixels[plus_ix]);
        accumulator = fma(plus, vec4<f32>(weight), accumulator);
        let minus = filter_blur_channels(shared_blur_pixels[minus_ix]);
        accumulator = fma(minus, vec4<f32>(weight), accumulator);
        weight *= weight_ratio;
        weight_ratio *= weight_ratio_decay;
        d += 1i;
    }

    return filter_blur_pack_average(accumulator, sum);
}

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn filter_blur_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = filter_region_index(gid);
    if (!filter_region_ix_valid(region_ix)) {
        return;
    }

    let std_dev = max(config.amount, 0.0);
    let xy = xy_for_region_ix(region_ix);
    let dst_ix = target_ix_at(xy.x, xy.y);
    if (std_dev <= 0.0) {
        target_store_ix(dst_ix, source_pixel_ix(dst_ix));
        return;
    }

    target_store_ix(dst_ix, filter_blur_pixel(xy, dst_ix, std_dev));
}

@compute @workgroup_size(SHARED_BLUR_TILE_WIDTH, SHARED_BLUR_TILE_HEIGHT)
fn filter_blur_shared_region(
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
) {
    let std_dev = max(config.amount, 0.0);
    var tile_x0 = config.region_x0 + workgroup_id.x * SHARED_BLUR_TILE_WIDTH;
    var tile_y0 = config.region_y0 + workgroup_id.y * SHARED_BLUR_TILE_HEIGHT;
    if (config.compact_tiles != 0u) {
        let dispatch_ix = workgroup_id.x + workgroup_id.y * config.dispatch_width;
        if (dispatch_ix >= config.active_tile_count) {
            return;
        }
        let tile = active_tiles[dispatch_ix];
        tile_x0 = (tile % config.tiles_width) * SHARED_BLUR_TILE_WIDTH;
        tile_y0 = (tile / config.tiles_width) * SHARED_BLUR_TILE_HEIGHT;
    }
    let xy = vec2<u32>(tile_x0 + local_id.x, tile_y0 + local_id.y);
    let in_region = xy.x < config.width &&
        xy.y < config.height &&
        xy.x >= config.region_x0 &&
        xy.y >= config.region_y0 &&
        xy.x < config.region_x0 + config.region_width &&
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
            target_store_ix(ix, filter_blur_pixel(xy, ix, std_dev));
        }
        return;
    }

    let radius = u32(half_width);
    let local_ix = local_id.y * SHARED_BLUR_TILE_WIDTH + local_id.x;
    let region_x0 = i32(filter_blur_sample_x0());
    let region_y0 = i32(filter_blur_sample_y0());
    let region_x1 = i32(filter_blur_sample_x1());
    let region_y1 = i32(filter_blur_sample_y1());
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
            sx >= region_x0 &&
            sx < region_x1 &&
            sy >= region_y0 &&
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

@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
