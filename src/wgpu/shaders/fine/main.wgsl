@compute @workgroup_size(256)
fn fine_tile_main(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = workgroup_id.x;
    if (tile_ix >= config.tile_count) {
        return;
    }

    let local_ix = local_id.x;
    let local_x = local_ix % 16u;
    let local_y = local_ix / 16u;
    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    if (tile_y >= config.tiles_height) {
        return;
    }

    let global_x = tile_x * 16u + local_x;
    let global_y = tile_y * 16u + local_y;
    if (global_x >= config.width || global_y >= config.height) {
        return;
    }

    target_store(global_x, global_y, tile_pixel(tile_ix, local_ix));
}

fn tile_pixel(tile_ix: u32, local_ix: u32) -> u32 {
    let local_x = local_ix % 16u;
    let local_y = local_ix / 16u;
    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    let global_x = tile_x * 16u + local_x;
    let global_y = tile_y * 16u + local_y;
    var pixel = config.clear_color;
    if (config.load_target != 0u) {
        pixel = target_load(global_x, global_y);
    }
    var clip_mask = 255u;
    var clip_depth = 0u;
    var clip_stack0 = 255u;
    var clip_stack1 = 255u;
    var clip_stack2 = 255u;
    var clip_stack3 = 255u;
    var group_depth = 0u;
    var group0_kind = 0u;
    var group0_parent_pixel = 0u;
    var group0_parent_clip = 0u;
    var group0_layer_alpha = 0u;
    var group0_payload = 0u;
    var group1_kind = 0u;
    var group1_parent_pixel = 0u;
    var group1_parent_clip = 0u;
    var group1_layer_alpha = 0u;
    var group1_payload = 0u;

    let tile = coarse_load_tile(tile_ix);
    var ptcl_ix = tile.ptcl_start;
    let range_end = tile.ptcl_end;
    loop {
        if (ptcl_ix >= range_end) {
            break;
        }
        let ptcl = coarse_load_ptcl(ptcl_ix);
        let tag = ptcl.tag;
        if (tag == GPU_PTCL_END) {
            break;
        }

        if (tag == GPU_PTCL_COLOR) {
            let color = scale_premul_u8(ptcl.color, clip_mask);
            pixel = src_over_premul_u8(pixel, color);
        } else if (tag == GPU_PTCL_SDF) {
            let draw_ix = ptcl.color;
            let coverage = sdf_coverage_from_draw(
                draw_records[draw_ix],
                f32(global_x) + 0.5,
                f32(global_y) + 0.5,
            );
            let alpha = combine_alpha(coverage_to_u8(coverage), clip_mask);
            if (alpha != 0u) {
                let color = sample_brush(draw_records[draw_ix].brush_offset, f32(global_x) + 0.5, f32(global_y) + 0.5);
                pixel = src_over_premul_u8(pixel, scale_premul_u8(color, alpha));
            }
        } else if (tag == GPU_PTCL_GLYPH) {
            pixel = composite_glyphs_at(
                pixel,
                ptcl.segment_start,
                ptcl.segment_end,
                ptcl.color,
                global_x,
                global_y,
                clip_mask,
            );
        } else if (tag == GPU_PTCL_END_CLIP) {
            if (clip_depth > 0u) {
                clip_depth -= 1u;
                if (clip_depth == 0u) {
                    clip_mask = clip_stack0;
                } else if (clip_depth == 1u) {
                    clip_mask = clip_stack1;
                } else if (clip_depth == 2u) {
                    clip_mask = clip_stack2;
                } else if (clip_depth == 3u) {
                    clip_mask = clip_stack3;
                } else {
                    let spill_depth_ix = clip_depth - FINE_LOCAL_CLIP_DEPTH;
                    if (spill_depth_ix < config.clip_spill_depth) {
                        let stack_ix =
                            (tile_ix * config.clip_spill_depth + spill_depth_ix) *
                            FINE_WORKGROUP_SIZE +
                            local_ix;
                        clip_mask = spills[stack_ix];
                    }
                }
            } else {
                clip_mask = 255u;
            }
        } else if (tag == GPU_PTCL_BEGIN_SDF_CLIP) {
            let draw_ix = ptcl.color;
            let alpha = coverage_to_u8(sdf_coverage_from_draw(
                draw_records[draw_ix],
                f32(global_x) + 0.5,
                f32(global_y) + 0.5,
            ));
            push_clip(
                clip_mask,
                tile_ix,
                local_ix,
                &clip_depth,
                &clip_stack0,
                &clip_stack1,
                &clip_stack2,
                &clip_stack3,
            );
            clip_mask = combine_alpha(clip_mask, alpha);
        } else if (tag == GPU_PTCL_END_OPACITY || tag == GPU_PTCL_END_BLEND) {
            if (group_depth > 0u) {
                group_depth -= 1u;
                var parent = 0u;
                var parent_clip = 0u;
                var layer_alpha = 0u;
                var payload = 0u;
                var group_kind = 0u;
                if (group_depth == 0u) {
                    parent = group0_parent_pixel;
                    parent_clip = group0_parent_clip;
                    layer_alpha = group0_layer_alpha;
                    payload = group0_payload;
                    group_kind = group0_kind;
                } else if (group_depth == 1u) {
                    parent = group1_parent_pixel;
                    parent_clip = group1_parent_clip;
                    layer_alpha = group1_layer_alpha;
                    payload = group1_payload;
                    group_kind = group1_kind;
                } else {
                    let spill_depth_ix = group_depth - FINE_LOCAL_GROUP_DEPTH;
                    if (spill_depth_ix < config.group_spill_depth) {
                        let stack_ix =
                            ((tile_ix * config.group_spill_depth + spill_depth_ix) *
                                FINE_WORKGROUP_SIZE +
                                local_ix) *
                            FINE_GROUP_SPILL_FIELDS;
                        let spill_base = config.group_spill_base + stack_ix;
                        group_kind = spills[spill_base];
                        parent = spills[spill_base + 1u];
                        parent_clip = spills[spill_base + 2u];
                        layer_alpha = spills[spill_base + 3u];
                        payload = spills[spill_base + 4u];
                    }
                }
                if (group_kind == GPU_PTCL_BEGIN_OPACITY) {
                    let alpha = combine_alpha(combine_alpha(layer_alpha, parent_clip), payload);
                    pixel = src_over_premul_u8(parent, scale_premul_u8(pixel, alpha));
                } else if (group_kind == GPU_PTCL_BEGIN_BLEND) {
                    let alpha = combine_alpha(layer_alpha, parent_clip);
                    let src = scale_premul_u8(pixel, alpha);
                    if ((src >> 24u) == 0u) {
                        pixel = parent;
                    } else {
                        pixel = blend_premul_u8(parent, src, payload);
                    }
                }
            }
        } else if (
            tag == GPU_PTCL_FILL ||
            tag == GPU_PTCL_PATH_GLYPH ||
            tag == GPU_PTCL_BEGIN_CLIP ||
            tag == GPU_PTCL_BEGIN_OPACITY ||
            tag == GPU_PTCL_BEGIN_BLEND
        ) {
            let alpha = fill_alpha_at(
                ptcl.backdrop,
                ptcl.fill_rule,
                ptcl.segment_start,
                ptcl.segment_end,
                local_x,
                local_y,
            );
            if (tag == GPU_PTCL_BEGIN_CLIP) {
                push_clip(
                    clip_mask,
                    tile_ix,
                    local_ix,
                    &clip_depth,
                    &clip_stack0,
                    &clip_stack1,
                    &clip_stack2,
                    &clip_stack3,
                );
                clip_mask = combine_alpha(clip_mask, alpha);
            } else if (tag == GPU_PTCL_BEGIN_OPACITY || tag == GPU_PTCL_BEGIN_BLEND) {
                var pushed_group = false;
                if (group_depth == 0u) {
                    group0_kind = tag;
                    group0_parent_pixel = pixel;
                    group0_parent_clip = clip_mask;
                    group0_layer_alpha = alpha;
                    group0_payload = ptcl.color;
                    pushed_group = true;
                } else if (group_depth == 1u) {
                    group1_kind = tag;
                    group1_parent_pixel = pixel;
                    group1_parent_clip = clip_mask;
                    group1_layer_alpha = alpha;
                    group1_payload = ptcl.color;
                    pushed_group = true;
                } else {
                    let spill_depth_ix = group_depth - FINE_LOCAL_GROUP_DEPTH;
                    if (spill_depth_ix < config.group_spill_depth) {
                        let stack_ix =
                            ((tile_ix * config.group_spill_depth + spill_depth_ix) *
                                FINE_WORKGROUP_SIZE +
                                local_ix) *
                            FINE_GROUP_SPILL_FIELDS;
                        let spill_base = config.group_spill_base + stack_ix;
                        spills[spill_base] = tag;
                        spills[spill_base + 1u] = pixel;
                        spills[spill_base + 2u] = clip_mask;
                        spills[spill_base + 3u] = alpha;
                        spills[spill_base + 4u] = ptcl.color;
                        pushed_group = true;
                    }
                }
                if (pushed_group) {
                    group_depth += 1u;
                    pixel = 0u;
                }
            } else {
                let masked_alpha = combine_alpha(alpha, clip_mask);
                if (masked_alpha != 0u) {
                    let draw_ix = ptcl.color;
                    let color = sample_brush(draw_records[draw_ix].brush_offset, f32(global_x) + 0.5, f32(global_y) + 0.5);
                    if (tag == GPU_PTCL_PATH_GLYPH) {
                        pixel = src_over_mask_linear_auto_u8(pixel, color, masked_alpha);
                    } else {
                        pixel = src_over_premul_u8(pixel, scale_premul_u8(color, masked_alpha));
                    }
                }
            }
        }
        ptcl_ix += 1u;
    }

    return pixel;
}

fn composite_glyphs_at(
    start_pixel: u32,
    glyph_start: u32,
    glyph_end: u32,
    draw_ix: u32,
    global_x: u32,
    global_y: u32,
    clip_mask: u32,
) -> u32 {
    var pixel = start_pixel;
    var glyph_list_ix = glyph_start;
    let px = i32(global_x);
    let py = i32(global_y);
    loop {
        if (glyph_list_ix >= glyph_end) {
            break;
        }
        let glyph_i = coarse_load_glyph(glyph_list_ix);
        let glyph = glyph_at(glyph_i);
        let image_id = glyph.image_id;
        if (image_id != INVALID_REF) {
            let image = glyph_image_at(image_id);
            let width = image.width;
            let height = image.height;
            let x0 = glyph.x + image.left;
            let y0 = glyph.y - image.top;
            let local_x = px - x0;
            let local_y = py - y0;
            if (local_x >= 0i && local_y >= 0i && local_x < i32(width) && local_y < i32(height)) {
                let data_ix = image.data_offset + u32(local_y) * width + u32(local_x);
                let content = image.content;
                let data = glyph_image_data_at(data_ix);
                if (content == GPU_GLYPH_MASK) {
                    let alpha = combine_alpha(data, clip_mask);
                    if (alpha != 0u) {
                        let color = sample_brush(draw_records[draw_ix].brush_offset, f32(global_x) + 0.5, f32(global_y) + 0.5);
                        pixel = src_over_premul_u8(pixel, scale_premul_u8(color, alpha));
                    }
                } else if (content == GPU_GLYPH_LINEAR_MASK) {
                    let alpha = combine_alpha(data, clip_mask);
                    if (alpha != 0u) {
                        let color = sample_brush(draw_records[draw_ix].brush_offset, f32(global_x) + 0.5, f32(global_y) + 0.5);
                        pixel = src_over_mask_linear_auto_u8(pixel, color, alpha);
                    }
                } else if (content == GPU_GLYPH_COLOR) {
                    pixel = src_over_premul_u8(pixel, scale_premul_u8(data, clip_mask));
                } else if (content == GPU_GLYPH_LINEAR_COLOR) {
                    pixel = src_over_mask_linear_auto_u8(pixel, data, clip_mask);
                } else if (content == GPU_GLYPH_SUBPIXEL_MASK) {
                    let color = sample_brush(draw_records[draw_ix].brush_offset, f32(global_x) + 0.5, f32(global_y) + 0.5);
                    pixel = src_over_subpixel_mask_u8(pixel, color, data, clip_mask);
                } else if (content == GPU_GLYPH_LINEAR_SUBPIXEL_MASK) {
                    let color = sample_brush(draw_records[draw_ix].brush_offset, f32(global_x) + 0.5, f32(global_y) + 0.5);
                    pixel = src_over_subpixel_mask_linear_auto_u8(pixel, color, data, clip_mask);
                }
            }
        }
        glyph_list_ix += 1u;
    }
    return pixel;
}

fn push_clip(
    mask: u32,
    tile_ix: u32,
    lane_ix: u32,
    depth: ptr<function, u32>,
    stack0: ptr<function, u32>,
    stack1: ptr<function, u32>,
    stack2: ptr<function, u32>,
    stack3: ptr<function, u32>,
) {
    let d = *depth;
    if (d == 0u) {
        *stack0 = mask;
        *depth = 1u;
    } else if (d == 1u) {
        *stack1 = mask;
        *depth = 2u;
    } else if (d == 2u) {
        *stack2 = mask;
        *depth = 3u;
    } else if (d == 3u) {
        *stack3 = mask;
        *depth = 4u;
    } else {
        let spill_depth_ix = d - FINE_LOCAL_CLIP_DEPTH;
        if (spill_depth_ix < config.clip_spill_depth) {
            let stack_ix =
                (tile_ix * config.clip_spill_depth + spill_depth_ix) * FINE_WORKGROUP_SIZE +
                lane_ix;
            spills[stack_ix] = mask;
            *depth = d + 1u;
        }
    }
}

fn fill_alpha_at(
    backdrop: i32,
    fill_rule: u32,
    segment_start: u32,
    segment_end: u32,
    x: u32,
    y: u32,
) -> u32 {
    // Mirrors the CPU row-sweep accumulation order so edge pixels quantize identically.
    var base = f32(backdrop);
    var running = 0.0;
    var partial = 0.0;
    var segment_ix = segment_start;
    loop {
        if (segment_ix >= segment_end) {
            break;
        }
        let segment = segments[segment_ix];
        let parts = segment_row_parts(segment.p0x, segment.p0y, segment.p1x, segment.p1y, segment.y_edge, y);
        let y_edge = parts.x;
        let dy = parts.y;
        let xmin = parts.z;
        let xmax = parts.w;
        base += y_edge;
        if (dy != 0.0) {
            let full_start = clamp(i32(ceil(xmax)), 0, 16);
            if (full_start < 16 && i32(x) >= full_start) {
                running += dy;
            }
            let partial_start = clamp(i32(floor(xmin)), 0, 16);
            let partial_end = clamp(i32(ceil(xmax)), 0, 16);
            if (i32(x) >= partial_start && i32(x) < partial_end) {
                partial += segment_area_at(xmin, xmax, x) * dy;
            }
        }
        segment_ix += 1u;
    }
    let coverage = base + running + partial;
    return coverage_to_alpha(coverage, fill_rule);
}

fn segment_row_parts(p0x: f32, p0y: f32, p1x: f32, p1y: f32, y_edge: f32, y: u32) -> vec4<f32> {
    let delta_x = p1x - p0x;
    let delta_y = p1y - p0y;
    let row_y = f32(y);
    let local_y = p0y - row_y;
    let y0 = clamp(local_y, 0.0, 1.0);
    let y1 = clamp(local_y + delta_y, 0.0, 1.0);
    let dy = y0 - y1;
    let x_sign = signum_f32(delta_x);
    let row_edge = x_sign * clamp(row_y - y_edge + 1.0, 0.0, 1.0);

    if (dy == 0.0) {
        return vec4<f32>(row_edge, dy, 0.0, 0.0);
    }

    let recip = 1.0 / delta_y;
    let t0 = (y0 - local_y) * recip;
    let t1 = (y1 - local_y) * recip;
    let sx0 = p0x + t0 * delta_x;
    let sx1 = p0x + t1 * delta_x;
    return vec4<f32>(row_edge, dy, min(sx0, sx1), max(sx0, sx1));
}

fn segment_area_at(xmin_abs: f32, xmax_abs: f32, x: u32) -> f32 {
    let pixel_x = f32(x);
    let xmin = xmin_abs - pixel_x;
    let xmax = xmax_abs - pixel_x;
    var area = clamp(1.0 - xmin, 0.0, 1.0);
    if (xmax - xmin > 0.000001) {
        let a_min = min(xmin, 1.0) - 0.000001;
        let b = min(xmax, 1.0);
        let c = max(b, 0.0);
        let d = max(a_min, 0.0);
        area = (b + 0.5 * (d * d - c * c) - a_min) / (xmax - a_min);
    }
    return area;
}


