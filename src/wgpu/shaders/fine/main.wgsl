@compute @workgroup_size(256)
fn fine_tile_sdf_list_main(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = fine_tile_list_at(FINE_TILE_LIST_SDF, workgroup_id.x);
    render_list_tile(tile_ix, local_id.x, FINE_TILE_KIND_PURE_SDF_SOLID_NO_STACK);
}

@compute @workgroup_size(256)
fn fine_tile_mixed_list_main(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = fine_tile_list_at(FINE_TILE_LIST_MIXED, workgroup_id.x);
    render_list_tile(tile_ix, local_id.x, FINE_TILE_KIND_MIXED_ANALYTIC_SOLID_NO_STACK);
}

@compute @workgroup_size(256)
fn fine_tile_full_list_main(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = fine_tile_list_at(FINE_TILE_LIST_FULL, workgroup_id.x);
    render_list_tile(tile_ix, local_id.x, FINE_TILE_KIND_FULL_INTERPRETER);
}

@compute @workgroup_size(256)
fn fine_tile_main(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    if (workgroup_id.x >= config.active_tile_count) {
        return;
    }
    let tile_ix = dispatched_tile_at(workgroup_id.x);

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

    let kind = fine_tile_kind_at(tile_ix);
    var pixel = vec4<f32>(0.0);
    if (kind == FINE_TILE_KIND_EMPTY_OR_CLEAR) {
        pixel = fine_initial_pixel(global_x, global_y);
    } else if (kind == FINE_TILE_KIND_COLOR_ONLY_NO_STACK) {
        pixel = color_only_no_stack_tile_pixel(tile_ix, local_ix);
    } else if (
        kind == FINE_TILE_KIND_PURE_SDF_SOLID_NO_STACK ||
        kind == FINE_TILE_KIND_MIXED_ANALYTIC_SOLID_NO_STACK
    ) {
        pixel = analytic_solid_no_stack_tile_pixel(tile_ix, local_ix);
    } else {
        pixel = tile_pixel(tile_ix, local_ix);
    }
    target_store_unorm(global_x, global_y, pixel);
}

fn render_list_tile(tile_ix: u32, local_ix: u32, kind: u32) {
    if (tile_ix >= config.tile_count) {
        return;
    }
    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    if (tile_y >= config.tiles_height) {
        return;
    }
    let global_x = tile_x * 16u + local_ix % 16u;
    let global_y = tile_y * 16u + local_ix / 16u;
    if (global_x >= config.width || global_y >= config.height) {
        return;
    }

    var pixel = vec4<f32>(0.0);
    if (
        kind == FINE_TILE_KIND_PURE_SDF_SOLID_NO_STACK ||
        kind == FINE_TILE_KIND_MIXED_ANALYTIC_SOLID_NO_STACK
    ) {
        pixel = analytic_solid_no_stack_tile_pixel(tile_ix, local_ix);
    } else {
        pixel = tile_pixel(tile_ix, local_ix);
    }
    target_store_unorm(global_x, global_y, pixel);
}

fn fine_initial_pixel(global_x: u32, global_y: u32) -> vec4<f32> {
    var pixel = rgba8_to_unorm(config.clear_color);
    if (config.load_target != 0u) {
        pixel = target_load_unorm(global_x, global_y);
    }
    return pixel;
}

fn color_only_no_stack_tile_pixel(tile_ix: u32, local_ix: u32) -> vec4<f32> {
    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    let global_x = tile_x * 16u + local_ix % 16u;
    let global_y = tile_y * 16u + local_ix / 16u;
    var pixel = fine_initial_pixel(global_x, global_y);

    let tile = coarse_load_tile(tile_ix);
    var ptcl_ix = tile.ptcl_start;
    loop {
        if (ptcl_ix >= tile.ptcl_end) {
            break;
        }
        let ptcl = coarse_load_ptcl(ptcl_ix);
        let tag = ptcl.tag;
        if (tag == GPU_PTCL_END) {
            break;
        }
        if (tag != GPU_PTCL_COLOR) {
            return tile_pixel(tile_ix, local_ix);
        }
        if (premul_u8_is_opaque(ptcl.color)) {
            pixel = rgba8_to_unorm(ptcl.color);
        } else {
            pixel = src_over_premul_unorm(pixel, rgba8_to_unorm(ptcl.color));
        }
        ptcl_ix += 1u;
    }
    return pixel;
}

fn analytic_solid_no_stack_tile_pixel(tile_ix: u32, local_ix: u32) -> vec4<f32> {
    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    let global_x = tile_x * 16u + local_ix % 16u;
    let global_y = tile_y * 16u + local_ix / 16u;
    let sample_x = f32(global_x) + 0.5;
    let sample_y = f32(global_y) + 0.5;
    var pixel = fine_initial_pixel(global_x, global_y);

    let tile = coarse_load_tile(tile_ix);
    if (tile.ptcl_start + 1u < tile.ptcl_end) {
        let first = coarse_load_ptcl(tile.ptcl_start);
        let second = coarse_load_ptcl(tile.ptcl_start + 1u);
        if (first.tag == GPU_PTCL_IMAGE && second.tag == GPU_PTCL_END) {
            let color = sample_image_draw_brush(draw_records[first.color], sample_x, sample_y);
            if (premul_u8_is_opaque(color)) {
                return rgba8_to_unorm(color);
            }
            return src_over_premul_unorm(pixel, rgba8_to_unorm(color));
        }
    }
    var ptcl_ix = tile.ptcl_start;
    loop {
        if (ptcl_ix >= tile.ptcl_end) {
            break;
        }
        let ptcl = coarse_load_ptcl(ptcl_ix);
        let tag = ptcl.tag;
        if (tag == GPU_PTCL_END) {
            break;
        }
        if (tag == GPU_PTCL_COLOR) {
            if (premul_u8_is_opaque(ptcl.color)) {
                pixel = rgba8_to_unorm(ptcl.color);
            } else {
                pixel = src_over_premul_unorm(pixel, rgba8_to_unorm(ptcl.color));
            }
        } else if (tag == GPU_PTCL_IMAGE) {
            let draw = draw_records[ptcl.color];
            let color = sample_image_draw_brush(draw, sample_x, sample_y);
            if (premul_u8_is_opaque(color)) {
                pixel = rgba8_to_unorm(color);
            } else {
                pixel = src_over_premul_unorm(pixel, rgba8_to_unorm(color));
            }
        } else if (tag == GPU_PTCL_SDF) {
            let draw = draw_records[ptcl.color];
            var coverage = 0.0;
            if (pixel_in_draw_bounds(draw, global_x, global_y)) {
                coverage = supported_sdf_coverage_from_draw(draw, sample_x, sample_y);
            }
            let alpha = coverage_to_u8(coverage);
            if (alpha != 0u) {
                let color = brush_word(draw.brush_offset + 4u);
                if (alpha == 255u && premul_u8_is_opaque(color)) {
                    pixel = rgba8_to_unorm(color);
                } else {
                    pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(color, alpha));
                }
            }
        } else {
            return tile_pixel(tile_ix, local_ix);
        }
        ptcl_ix += 1u;
    }
    return pixel;
}

fn tile_pixel(tile_ix: u32, local_ix: u32) -> vec4<f32> {
    let local_x = local_ix % 16u;
    let local_y = local_ix / 16u;
    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    let global_x = tile_x * 16u + local_x;
    let global_y = tile_y * 16u + local_y;
    // Fine can see many particles for the same pixel; keeping the accumulator in
    // premultiplied f32 avoids decode/pack work on the hot src-over paths.
    var pixel = rgba8_to_unorm(config.clear_color);
    if (config.load_target != 0u) {
        pixel = target_load_unorm(global_x, global_y);
    }
    var clip_mask = 255u;
    var clip_depth = 0u;
    var clip_stack0 = 255u;
    var clip_stack1 = 255u;
    var clip_stack2 = 255u;
    var clip_stack3 = 255u;
    var group_depth = 0u;
    var group0_kind = 0u;
    var group0_parent_pixel = vec4<f32>(0.0);
    var group0_parent_clip = 0u;
    var group0_layer_alpha = 0u;
    var group0_payload = 0u;
    var group1_kind = 0u;
    var group1_parent_pixel = vec4<f32>(0.0);
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
            if (clip_mask != 0u) {
                if (clip_mask == 255u && premul_u8_is_opaque(ptcl.color)) {
                    pixel = rgba8_to_unorm(ptcl.color);
                } else {
                    pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(ptcl.color, clip_mask));
                }
            }
        } else if (tag == GPU_PTCL_IMAGE) {
            if (clip_mask != 0u) {
                let draw = draw_records[ptcl.color];
                let color = sample_image_draw_brush(draw, f32(global_x) + 0.5, f32(global_y) + 0.5);
                if (clip_mask == 255u && premul_u8_is_opaque(color)) {
                    pixel = rgba8_to_unorm(color);
                } else {
                    pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(color, clip_mask));
                }
            }
        } else if (tag == GPU_PTCL_SDF) {
            if (clip_mask != 0u) {
                let draw_ix = ptcl.color;
                let draw = draw_records[draw_ix];
                let coverage = sdf_coverage_from_draw(
                    draw,
                    f32(global_x) + 0.5,
                    f32(global_y) + 0.5,
                );
                let alpha = combine_alpha(coverage_to_u8(coverage), clip_mask);
                if (alpha != 0u) {
                    let color = sample_draw_brush(draw, f32(global_x) + 0.5, f32(global_y) + 0.5);
                    if (alpha == 255u && premul_u8_is_opaque(color)) {
                        pixel = rgba8_to_unorm(color);
                    } else {
                        pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(color, alpha));
                    }
                }
            }
        } else if (tag == GPU_PTCL_GLYPH) {
            let draw = draw_records[ptcl.color];
            // Coarse bounds select whole 16px tiles; enforce the exact glyph draw domain here so
            // an edge tile cannot leak past a draw-local text clip.
            if (clip_mask != 0u && pixel_in_draw_bounds(draw, global_x, global_y)) {
                pixel = composite_glyphs_at(
                    pixel,
                    ptcl.segment_start,
                    ptcl.segment_end,
                    draw,
                    global_x,
                    global_y,
                    clip_mask,
                );
            }
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
            let parent_clip = clip_mask;
            var alpha = 0u;
            if (parent_clip != 0u) {
                alpha = coverage_to_u8(sdf_coverage_from_draw(
                    draw_records[draw_ix],
                    f32(global_x) + 0.5,
                    f32(global_y) + 0.5,
                ));
            }
            push_clip(
                parent_clip,
                tile_ix,
                local_ix,
                &clip_depth,
                &clip_stack0,
                &clip_stack1,
                &clip_stack2,
                &clip_stack3,
            );
            clip_mask = combine_alpha(parent_clip, alpha);
        } else if (tag == GPU_PTCL_END_OPACITY || tag == GPU_PTCL_END_BLEND) {
            if (group_depth > 0u) {
                group_depth -= 1u;
                var parent = vec4<f32>(0.0);
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
                        parent = rgba8_to_unorm(spills[spill_base + 1u]);
                        parent_clip = spills[spill_base + 2u];
                        layer_alpha = spills[spill_base + 3u];
                        payload = spills[spill_base + 4u];
                    }
                }
                if (group_kind == GPU_PTCL_BEGIN_OPACITY) {
                    let alpha = combine_alpha(combine_alpha(layer_alpha, parent_clip), payload);
                    pixel = src_over_premul_unorm(parent, scale_premul_u8_to_unorm(unorm_to_rgba8(pixel), alpha));
                } else if (group_kind == GPU_PTCL_BEGIN_BLEND) {
                    let alpha = combine_alpha(layer_alpha, parent_clip);
                    let src = scale_premul_u8(unorm_to_rgba8(pixel), alpha);
                    if ((src >> 24u) == 0u) {
                        pixel = parent;
                    } else {
                        pixel = rgba8_to_unorm(blend_premul_u8(unorm_to_rgba8(parent), src, payload));
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
                        spills[spill_base + 1u] = unorm_to_rgba8(pixel);
                        spills[spill_base + 2u] = clip_mask;
                        spills[spill_base + 3u] = alpha;
                        spills[spill_base + 4u] = ptcl.color;
                        pushed_group = true;
                    }
                }
                if (pushed_group) {
                    group_depth += 1u;
                    pixel = vec4<f32>(0.0);
                }
            } else {
                let masked_alpha = combine_alpha(alpha, clip_mask);
                if (masked_alpha != 0u) {
                    let draw_ix = ptcl.color;
                    let draw = draw_records[draw_ix];
                    let color = sample_draw_brush(draw, f32(global_x) + 0.5, f32(global_y) + 0.5);
                    if (tag == GPU_PTCL_PATH_GLYPH) {
                        pixel = rgba8_to_unorm(src_over_mask_linear_auto_u8(unorm_to_rgba8(pixel), color, masked_alpha));
                    } else if (masked_alpha == 255u && premul_u8_is_opaque(color)) {
                        pixel = rgba8_to_unorm(color);
                    } else {
                        pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(color, masked_alpha));
                    }
                }
            }
        }
        ptcl_ix += 1u;
    }

    return pixel;
}

fn premul_u8_is_opaque(pixel: u32) -> bool {
    return (pixel >> 24u) == 255u;
}

fn sample_draw_brush(draw: DrawRecord, x: f32, y: f32) -> u32 {
    let brush_offset = draw.brush_offset;
    if (brush_word(brush_offset) == GPU_BRUSH_SOLID) {
        return brush_word(brush_offset + 4u);
    }
    let local = affine_record_point(draw.inverse_transform, vec2<f32>(x, y));
    return sample_brush(brush_offset, local.x, local.y);
}

fn sample_image_draw_brush(draw: DrawRecord, x: f32, y: f32) -> u32 {
    let local = affine_record_point(draw.inverse_transform, vec2<f32>(x, y));
    let data_base = draw.brush_offset;
    let base = data_base + GPU_BRUSH_U32_STRIDE;
    return sample_resource_pattern(
        local.x,
        local.y,
        base,
        brush_word(data_base + 4u),
        brush_word(data_base + 2u),
        brush_word(data_base + 3u),
        brush_word(data_base + 5u),
        brush_word(data_base + 6u),
        brush_word(data_base + 7u),
        brush_word(data_base + 1u),
        brush_word(data_base + 8u),
    );
}

fn pixel_in_draw_bounds(draw: DrawRecord, global_x: u32, global_y: u32) -> bool {
    let px = i32(global_x);
    let py = i32(global_y);
    return px >= draw.pixel_x0 && py >= draw.pixel_y0 && px < draw.pixel_x1 && py < draw.pixel_y1;
}

fn supported_sdf_coverage_from_draw(draw: DrawRecord, x: f32, y: f32) -> f32 {
    return sdf_coverage_from_draw(draw, x, y);
}

fn composite_glyphs_at(
    start_pixel: vec4<f32>,
    glyph_start: u32,
    glyph_end: u32,
    draw: DrawRecord,
    global_x: u32,
    global_y: u32,
    clip_mask: u32,
) -> vec4<f32> {
    var pixel = start_pixel;
    var glyph_list_ix = glyph_start;
    let local = affine_record_point(
        draw.inverse_transform,
        vec2<f32>(f32(global_x) + 0.5, f32(global_y) + 0.5),
    );
    let px = i32(floor(local.x));
    let py = i32(floor(local.y));
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
                        let color = sample_draw_brush(draw, f32(global_x) + 0.5, f32(global_y) + 0.5);
                        pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(color, alpha));
                    }
                } else if (content == GPU_GLYPH_LINEAR_MASK) {
                    let alpha = combine_alpha(data, clip_mask);
                    if (alpha != 0u) {
                        let color = sample_draw_brush(draw, f32(global_x) + 0.5, f32(global_y) + 0.5);
                        pixel = rgba8_to_unorm(src_over_mask_linear_auto_u8(unorm_to_rgba8(pixel), color, alpha));
                    }
                } else if (content == GPU_GLYPH_COLOR) {
                    pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(data, clip_mask));
                } else if (content == GPU_GLYPH_LINEAR_COLOR) {
                    pixel = rgba8_to_unorm(src_over_mask_linear_auto_u8(unorm_to_rgba8(pixel), data, clip_mask));
                } else if (content == GPU_GLYPH_SUBPIXEL_MASK) {
                    let color = sample_draw_brush(draw, f32(global_x) + 0.5, f32(global_y) + 0.5);
                    pixel = rgba8_to_unorm(src_over_subpixel_mask_u8(unorm_to_rgba8(pixel), color, data, clip_mask));
                } else if (content == GPU_GLYPH_LINEAR_SUBPIXEL_MASK) {
                    let color = sample_draw_brush(draw, f32(global_x) + 0.5, f32(global_y) + 0.5);
                    pixel = rgba8_to_unorm(src_over_subpixel_mask_linear_auto_u8(unorm_to_rgba8(pixel), color, data, clip_mask));
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
    if (xmax - xmin > FINE_AREA_EPSILON) {
        let a_min = min(xmin, 1.0) - FINE_AREA_EPSILON;
        let b = min(xmax, 1.0);
        let c = max(b, 0.0);
        let d = max(a_min, 0.0);
        area = (b + 0.5 * (d * d - c * c) - a_min) / (xmax - a_min);
    }
    return area;
}
