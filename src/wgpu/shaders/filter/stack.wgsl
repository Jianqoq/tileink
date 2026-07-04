fn composite_with_stack(dst: u32, source: u32, mask: u32, x: u32, y: u32, force_blend: bool) -> u32 {
    var pixel = dst;
    var clip_mask = 255u;
    var group_depth = 0u;
    var group_kinds: array<u32, 64>;
    var group_parent_pixels: array<u32, 64>;
    var group_parent_clips: array<u32, 64>;
    var group_layer_alphas: array<u32, 64>;
    var group_payloads: array<u32, 64>;

    var stack_ix = config.layer_stack_start;
    loop {
        if (stack_ix >= config.layer_stack_end) {
            break;
        }
        let layer = layer_stack[stack_ix];
        let tag = layer.tag;
        let alpha = layer_stack_alpha_at(layer.draw, x, y);
        if (tag == GPU_LAYER_CLIP) {
            clip_mask = combine_alpha(clip_mask, alpha);
        } else if (tag == GPU_LAYER_OPACITY || tag == GPU_LAYER_BLEND) {
            if (group_depth < FILTER_GROUP_STACK_CAPACITY) {
                group_kinds[group_depth] = tag;
                group_parent_pixels[group_depth] = pixel;
                group_parent_clips[group_depth] = clip_mask;
                group_layer_alphas[group_depth] = alpha;
                group_payloads[group_depth] = layer.payload;
                group_depth += 1u;
                pixel = 0u;
            }
        }
        stack_ix += 1u;
    }

    var source_alpha = clip_mask;
    if (config.mask_enabled != 0u || force_blend) {
        source_alpha = combine_alpha(source_alpha, mask >> 24u);
    }
    let scaled_source = scale_premul_u8(source, source_alpha);
    if (force_blend) {
        if ((scaled_source >> 24u) != 0u) {
            pixel = blend_premul_u8(pixel, scaled_source, config.blend_mode);
        }
    } else {
        pixel = src_over_premul_u8(pixel, scaled_source);
    }

    loop {
        if (group_depth == 0u) {
            break;
        }
        group_depth -= 1u;
        let parent = group_parent_pixels[group_depth];
        let parent_clip = group_parent_clips[group_depth];
        let layer_alpha = group_layer_alphas[group_depth];
        let payload = group_payloads[group_depth];
        let group_kind = group_kinds[group_depth];
        var alpha = combine_alpha(layer_alpha, parent_clip);
        if (group_kind == GPU_LAYER_OPACITY) {
            alpha = combine_alpha(alpha, payload);
            pixel = src_over_premul_u8(parent, scale_premul_u8(pixel, alpha));
        } else if (group_kind == GPU_LAYER_BLEND) {
            let src = scale_premul_u8(pixel, alpha);
            if ((src >> 24u) == 0u) {
                pixel = parent;
            } else {
                pixel = blend_premul_u8(parent, src, payload);
            }
        }
    }

    return pixel;
}

fn composite_surface_with_stack(dst: u32, source: u32, x: u32, y: u32) -> u32 {
    var pixel = dst;
    var clip_mask = 255u;
    var group_depth = 0u;
    var group_kinds: array<u32, 64>;
    var group_parent_pixels: array<u32, 64>;
    var group_parent_clips: array<u32, 64>;
    var group_layer_alphas: array<u32, 64>;
    var group_payloads: array<u32, 64>;

    var stack_ix = config.layer_stack_start;
    loop {
        if (stack_ix >= config.layer_stack_end) {
            break;
        }
        let layer = layer_stack[stack_ix];
        let tag = layer.tag;
        let alpha = layer_stack_alpha_at(layer.draw, x, y);
        if (tag == GPU_LAYER_CLIP) {
            clip_mask = combine_alpha(clip_mask, alpha);
        } else if (tag == GPU_LAYER_OPACITY || tag == GPU_LAYER_BLEND) {
            if (group_depth < FILTER_GROUP_STACK_CAPACITY) {
                group_kinds[group_depth] = tag;
                group_parent_pixels[group_depth] = pixel;
                group_parent_clips[group_depth] = clip_mask;
                group_layer_alphas[group_depth] = alpha;
                group_payloads[group_depth] = layer.payload;
                group_depth += 1u;
                pixel = 0u;
            }
        }
        stack_ix += 1u;
    }

    pixel = src_over_premul_u8(pixel, scale_premul_u8(source, clip_mask));

    loop {
        if (group_depth == 0u) {
            break;
        }
        group_depth -= 1u;
        let parent = group_parent_pixels[group_depth];
        let parent_clip = group_parent_clips[group_depth];
        let layer_alpha = group_layer_alphas[group_depth];
        let payload = group_payloads[group_depth];
        let group_kind = group_kinds[group_depth];
        var alpha = combine_alpha(layer_alpha, parent_clip);
        if (group_kind == GPU_LAYER_OPACITY) {
            alpha = combine_alpha(alpha, payload);
            pixel = src_over_premul_u8(parent, scale_premul_u8(pixel, alpha));
        } else if (group_kind == GPU_LAYER_BLEND) {
            let src = scale_premul_u8(pixel, alpha);
            if ((src >> 24u) == 0u) {
                pixel = parent;
            } else {
                pixel = blend_premul_u8(parent, src, payload);
            }
        }
    }

    return pixel;
}

fn xy_for_region_ix(region_ix: u32) -> vec2<u32> {
    return vec2<u32>(
        config.region_x0 + region_ix % config.region_width,
        config.region_y0 + region_ix / config.region_width,
    );
}

fn target_ix_for_region_ix(region_ix: u32) -> u32 {
    let xy = xy_for_region_ix(region_ix);
    return target_ix_at(xy.x, xy.y);
}

fn target_ix_at(x: u32, y: u32) -> u32 {
    return y * config.width + x;
}

fn xy_for_target_ix(ix: u32) -> vec2<u32> {
    return vec2<u32>(ix % config.width, ix / config.width);
}

fn source_pixel_at(x: u32, y: u32) -> u32 {
    return filter_source_load(x, y);
}

fn source_pixel_ix(ix: u32) -> u32 {
    let xy = xy_for_target_ix(ix);
    return source_pixel_at(xy.x, xy.y);
}

fn aux_pixel_at(x: u32, y: u32) -> u32 {
    return filter_aux_load(x, y);
}

fn aux_pixel_ix(ix: u32) -> u32 {
    let xy = xy_for_target_ix(ix);
    return aux_pixel_at(xy.x, xy.y);
}

fn target_load_at(x: u32, y: u32) -> u32 {
    return filter_target_load(x, y);
}

fn target_load_ix(ix: u32) -> u32 {
    let xy = xy_for_target_ix(ix);
    return target_load_at(xy.x, xy.y);
}

fn target_store_at(x: u32, y: u32, pixel: u32) {
    filter_target_store(x, y, pixel);
}

fn target_store_ix(ix: u32, pixel: u32) {
    let xy = xy_for_target_ix(ix);
    target_store_at(xy.x, xy.y, pixel);
}

fn gray_alpha(alpha: u32) -> u32 {
    return alpha | (alpha << 8u) | (alpha << 16u) | (alpha << 24u);
}

fn layer_stack_alpha_at(draw_ix: u32, x: u32, y: u32) -> u32 {
    let draw_i = draw_ix;
    var alpha = 0u;
    if (draw_i >= arrayLength(&draw_records)) {
        return alpha;
    }
    let global_x = i32(x);
    let global_y = i32(y);
    let draw = draw_records[draw_i];
    if (draw.sdf_offset != INVALID || draw.sdf_shadow_offset != INVALID) {
        if (
            global_x >= draw.pixel_x0 &&
            global_x < draw.pixel_x1 &&
            global_y >= draw.pixel_y0 &&
            global_y < draw.pixel_y1
        ) {
            alpha = coverage_to_u8(sdf_coverage_from_draw(draw, f32(x) + 0.5, f32(y) + 0.5));
        }
    } else {
        let tile_x = x / 16u;
        let tile_y = y / 16u;
        let local_x = x - tile_x * 16u;
        let local_y = y - tile_y * 16u;
        let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
        if (backdrop_ix != INVALID) {
            let segment_range = segment_ranges[backdrop_ix];
            alpha = fill_alpha_at(
                atomicLoad(&backdrops[backdrop_ix]),
                draw_fill_rule_at(draw_ix),
                segment_range.start,
                segment_range.end,
                local_x,
                local_y,
            );
        }
    }
    return alpha;
}

fn draw_backdrop_ix(draw_ix: u32, tile_x: u32, tile_y: u32) -> u32 {
    let draw = draw_records[draw_ix];
    let path_id = draw.path_id;
    let draw_tag = draw_tag_at(draw_ix);
    var result = INVALID;
    if (
        path_id != INVALID &&
        (draw_tag == GPU_DRAW_BRUSH ||
         draw_tag == GPU_DRAW_PATH_GLYPH ||
         draw_tag == GPU_DRAW_CLIP ||
         draw_tag == GPU_DRAW_OPACITY ||
         draw_tag == GPU_DRAW_BLEND ||
         draw_tag == GPU_DRAW_ISOLATE)
    ) {
        let draw_x0 = pixel_tile_min(draw.pixel_x0, config.tiles_width);
        let draw_y0 = pixel_tile_min(draw.pixel_y0, config.tiles_height);
        let draw_x1 = pixel_tile_max(draw.pixel_x1, config.tiles_width);
        let draw_y1 = pixel_tile_max(draw.pixel_y1, config.tiles_height);
        if (
            tile_x >= draw_x0 &&
            tile_x < draw_x1 &&
            tile_y >= draw_y0 &&
            tile_y < draw_y1 &&
            path_id < arrayLength(&path_records)
        ) {
            let path = path_records[path_id];
            let bx0 = path.tile_x0;
            let by0 = path.tile_y0;
            let bx1 = path.tile_x1;
            let by1 = path.tile_y1;
            let stride = bx1 - bx0;
            if (stride > 0u && tile_x >= bx0 && tile_x < bx1 && tile_y >= by0 && tile_y < by1) {
                result = path.data_offset + (tile_y - by0) * stride + tile_x - bx0;
            }
        }
    }
    return result;
}

fn pixel_tile_min(value: i32, limit: u32) -> u32 {
    var tile = 0u;
    if (value > 0i) {
        tile = min(u32(value) / 16u, limit);
    }
    return tile;
}

fn pixel_tile_max(value: i32, limit: u32) -> u32 {
    var tile = 0u;
    if (value > 0i) {
        tile = min((u32(value) + 15u) / 16u, limit);
    }
    return tile;
}

fn draw_tag_at(draw_ix: u32) -> u32 {
    return draw_records[draw_ix].tag;
}

fn draw_fill_rule_at(draw_ix: u32) -> u32 {
    return draw_records[draw_ix].fill_rule;
}

fn fill_alpha_at(backdrop: i32, fill_rule: u32, segment_start: u32, segment_end: u32, x: u32, y: u32) -> u32 {
    var coverage = f32(backdrop);
    var segment_ix = segment_start;
    loop {
        if (segment_ix >= segment_end) {
            break;
        }
        let segment = segments[segment_ix];
        coverage += segment_coverage_at(segment.p0x, segment.p0y, segment.p1x, segment.p1y, segment.y_edge, x, y);
        segment_ix += 1u;
    }
    return coverage_to_alpha(coverage, fill_rule);
}

fn segment_coverage_at(p0x: f32, p0y: f32, p1x: f32, p1y: f32, y_edge: f32, x: u32, y: u32) -> f32 {
    let delta_x = p1x - p0x;
    let delta_y = p1y - p0y;
    let row_y = f32(y);
    let local_y = p0y - row_y;
    let y0 = clamp(local_y, 0.0, 1.0);
    let y1 = clamp(local_y + delta_y, 0.0, 1.0);
    let dy = y0 - y1;
    let x_sign = signum_f32(delta_x);
    var coverage = x_sign * clamp(row_y - y_edge + 1.0, 0.0, 1.0);

    if (dy != 0.0) {
        let recip = 1.0 / delta_y;
        let t0 = (y0 - local_y) * recip;
        let t1 = (y1 - local_y) * recip;
        let sx0 = p0x + t0 * delta_x;
        let sx1 = p0x + t1 * delta_x;
        let pixel_x = f32(x);
        let xmin = min(sx0, sx1) - pixel_x;
        let xmax = max(sx0, sx1) - pixel_x;
        var area = clamp(1.0 - xmin, 0.0, 1.0);
        if (xmax - xmin > 0.000001) {
            let a_min = min(xmin, 1.0) - 0.000001;
            let b = min(xmax, 1.0);
            let c = max(b, 0.0);
            let d = max(a_min, 0.0);
            area = (b + 0.5 * (d * d - c * c) - a_min) / (xmax - a_min);
        }
        coverage += area * dy;
    }

    return coverage;
}

