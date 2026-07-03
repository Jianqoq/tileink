const DRAW_FLAG_SOLID_RECT: u32 = 16u;
const DRAW_FLAG_HAS_SDF: u32 = 64u;
const INVALID_REF: u32 = 4294967295u;

const GPU_SDF_RECT: u32 = 1u;
const GPU_SDF_CIRCLE: u32 = 2u;
const GPU_SDF_RECT_STROKE: u32 = 3u;
const GPU_SDF_CIRCLE_STROKE: u32 = 4u;
const GPU_SDF_CANDLESTICK: u32 = 5u;
const GPU_SDF_LINE: u32 = 6u;
const GPU_SDF_RECT_SHADOW: u32 = 7u;
const GPU_SDF_ARC: u32 = 8u;
const GPU_SDF_ARC_SHADOW: u32 = 9u;
const GPU_SDF_CIRCLE_SHADOW: u32 = 10u;
const GPU_SDF_LINE_SHADOW: u32 = 11u;
const GPU_SDF_DASH_LINE: u32 = 12u;
const GPU_PTCL_END: u32 = 0u;
const GPU_PTCL_FILL: u32 = 1u;
const GPU_PTCL_COLOR: u32 = 2u;
const GPU_PTCL_BEGIN_CLIP: u32 = 3u;
const GPU_PTCL_END_CLIP: u32 = 4u;
const GPU_PTCL_BEGIN_OPACITY: u32 = 5u;
const GPU_PTCL_END_OPACITY: u32 = 6u;
const GPU_PTCL_BEGIN_BLEND: u32 = 7u;
const GPU_PTCL_END_BLEND: u32 = 8u;
const GPU_PTCL_SDF: u32 = 9u;
const GPU_PTCL_GLYPH: u32 = 10u;
const GPU_PTCL_PATH_GLYPH: u32 = 11u;
const GPU_PTCL_BEGIN_SDF_CLIP: u32 = 12u;
const GPU_GLYPH_MASK: u32 = 0u;
const GPU_GLYPH_COLOR: u32 = 1u;
const GPU_GLYPH_SUBPIXEL_MASK: u32 = 2u;
const GPU_GLYPH_LINEAR_MASK: u32 = 3u;
const GPU_GLYPH_LINEAR_COLOR: u32 = 4u;
const GPU_GLYPH_LINEAR_SUBPIXEL_MASK: u32 = 5u;
const FINE_LOCAL_CLIP_DEPTH: u32 = 4u;
const FINE_LOCAL_GROUP_DEPTH: u32 = 2u;
const FINE_GROUP_SPILL_FIELDS: u32 = 5u;
const FINE_WORKGROUP_SIZE: u32 = 256u;

const GPU_BRUSH_U32_STRIDE: u32 = 9u;
const GPU_BRUSH_PARAM_STRIDE: u32 = 12u;
const GPU_BRUSH_LINEAR: u32 = 2u;
const GPU_BRUSH_RADIAL: u32 = 3u;
const GPU_BRUSH_SWEEP: u32 = 4u;
const GPU_BRUSH_FOUR_CORNER: u32 = 5u;
const GPU_BRUSH_PATTERN: u32 = 6u;
const GPU_PATTERN_BILINEAR: u32 = 1u;
const GPU_EXTEND_REPEAT: u32 = 1u;
const GPU_EXTEND_REFLECT: u32 = 2u;

const TEXT_DARK_ON_LIGHT_COVERAGE_STRENGTH: f32 = 0.95;
const TEXT_DARK_ON_LIGHT_LUMA_BASE: f32 = 1.5728465;
const TEXT_DARK_ON_LIGHT_LUMA_TAPER: f32 = 1.15;
const TEXT_DARK_ON_LIGHT_CHROMA_BOOST: f32 = 0.3656558;
const TEXT_SOURCE_CHROMA_COVERAGE_BOOST: f32 = 0.0;
const TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT: f32 = 0.23100804;
const TEXT_LIGHT_ON_DARK_COVERAGE_REDUCTION: f32 = 0.20662805;
const TEXT_LIGHT_ON_DARK_BLACK_LUMA_LIMIT: f32 = 0.02875403;
const TEXT_LIGHT_ON_DARK_CHROMA_REDUCTION: f32 = 0.11479953;
const TEXT_LIGHT_ON_DARK_HIGH_LUMA_CHROMA_REDUCTION: f32 = 0.4492354;
const TEXT_LIGHT_ON_DARK_HIGH_LUMA_THRESHOLD: f32 = 0.26129702;
const TEXT_LIGHT_ON_COLORED_DARK_CHROMA_REDUCTION: f32 = 0.48728964;
const TEXT_LIGHT_ON_COLORED_DARK_LUMA_LIMIT: f32 = 0.11519971;
const TEXT_ALPHA_MASK_CHROMA_SCALE: f32 = 1.3566802;
const TEXT_SUBPIXEL_MASK_CHROMA_SCALE: f32 = 0.9483659;
const TEXT_ALPHA_MASK_APPARENT_AXIS_STRENGTH: f32 = 1.036124;
const TEXT_ALPHA_MASK_APPARENT_AXIS_LUMA_LIMIT: f32 = 0.6887328;
const TEXT_SUBPIXEL_MASK_APPARENT_AXIS_STRENGTH: f32 = 1.447765;
const TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT: f32 = 0.1682842;
const TEXT_ALPHA_MASK_LOW_LUMA_CHROMA_REDUCTION: f32 = 0.41302064;
const TEXT_ALPHA_MASK_LOW_LUMA_CONTRAST_LIMIT: f32 = 0.10123872;
const TEXT_SUBPIXEL_MASK_LOW_LUMA_CHROMA_REDUCTION: f32 = 0.0;
const TEXT_SUBPIXEL_MASK_LOW_LUMA_CONTRAST_LIMIT: f32 = 0.06385561;

struct FineConfig {
    width: u32,
    height: u32,
    draw_count: u32,
    clear_color: u32,
    tile_count: u32,
    tiles_width: u32,
    tiles_height: u32,
    load_target: u32,
    clip_spill_depth: u32,
    group_spill_depth: u32,
};

@group(0) @binding(0) var<uniform> config: FineConfig;
@group(0) @binding(1) var target_texture: texture_storage_2d<rgba8unorm, read_write>;
@group(0) @binding(2) var<storage, read> draw_flags: array<u32>;
@group(0) @binding(3) var<storage, read> draw_brush_colors: array<u32>;
@group(0) @binding(4) var<storage, read> draw_pixel_x0: array<i32>;
@group(0) @binding(5) var<storage, read> draw_pixel_y0: array<i32>;
@group(0) @binding(6) var<storage, read> draw_pixel_x1: array<i32>;
@group(0) @binding(7) var<storage, read> draw_pixel_y1: array<i32>;
@group(0) @binding(8) var<storage, read> draw_sdf_refs: array<u32>;
@group(0) @binding(9) var<storage, read> sdf_kinds: array<u32>;
@group(0) @binding(10) var<storage, read> sdf_x0: array<f32>;
@group(0) @binding(11) var<storage, read> sdf_y0: array<f32>;
@group(0) @binding(12) var<storage, read> sdf_x1: array<f32>;
@group(0) @binding(13) var<storage, read> sdf_y1: array<f32>;
@group(0) @binding(14) var<storage, read> sdf_r0: array<f32>;
@group(0) @binding(15) var<storage, read> sdf_r1: array<f32>;
@group(0) @binding(16) var<storage, read> sdf_r2: array<f32>;
@group(0) @binding(17) var<storage, read> sdf_r3: array<f32>;
@group(0) @binding(18) var<storage, read> sdf_stroke_top: array<f32>;
@group(0) @binding(19) var<storage, read> sdf_stroke_right: array<f32>;
@group(0) @binding(20) var<storage, read> sdf_stroke_bottom: array<f32>;
@group(0) @binding(21) var<storage, read> sdf_stroke_left: array<f32>;
@group(0) @binding(22) var<storage, read> sdf_shadow_offset_x: array<f32>;
@group(0) @binding(23) var<storage, read> sdf_shadow_offset_y: array<f32>;
@group(0) @binding(24) var<storage, read> sdf_shadow_expand: array<f32>;
@group(0) @binding(25) var<storage, read> sdf_shadow_intensity: array<f32>;
@group(0) @binding(26) var<storage, read> brush_data: array<u32>;
@group(0) @binding(27) var<storage, read> brush_params: array<f32>;
@group(0) @binding(28) var<storage, read> brush_payloads: array<u32>;
@group(0) @binding(29) var<storage, read> tile_range_starts: array<u32>;
@group(0) @binding(30) var<storage, read> tile_range_ends: array<u32>;
@group(0) @binding(31) var<storage, read> ptcl_tags: array<u32>;
@group(0) @binding(32) var<storage, read> ptcl_backdrops: array<i32>;
@group(0) @binding(33) var<storage, read> ptcl_fill_rules: array<u32>;
@group(0) @binding(34) var<storage, read> ptcl_segment_starts: array<u32>;
@group(0) @binding(35) var<storage, read> ptcl_segment_ends: array<u32>;
@group(0) @binding(36) var<storage, read> ptcl_colors: array<u32>;
@group(0) @binding(37) var<storage, read> segment_p0x: array<f32>;
@group(0) @binding(38) var<storage, read> segment_p0y: array<f32>;
@group(0) @binding(39) var<storage, read> segment_p1x: array<f32>;
@group(0) @binding(40) var<storage, read> segment_p1y: array<f32>;
@group(0) @binding(41) var<storage, read> segment_y_edge: array<f32>;
@group(0) @binding(42) var<storage, read> glyph_indices: array<u32>;
@group(0) @binding(43) var<storage, read> glyph_image_ids: array<u32>;
@group(0) @binding(44) var<storage, read> glyph_x: array<i32>;
@group(0) @binding(45) var<storage, read> glyph_y: array<i32>;
@group(0) @binding(46) var<storage, read> glyph_image_left: array<i32>;
@group(0) @binding(47) var<storage, read> glyph_image_top: array<i32>;
@group(0) @binding(48) var<storage, read> glyph_image_width: array<u32>;
@group(0) @binding(49) var<storage, read> glyph_image_height: array<u32>;
@group(0) @binding(50) var<storage, read> glyph_image_content: array<u32>;
@group(0) @binding(51) var<storage, read> glyph_image_data_offsets: array<u32>;
@group(0) @binding(52) var<storage, read> glyph_image_data: array<u32>;
@group(0) @binding(53) var<storage, read_write> clip_spills: array<u32>;
@group(0) @binding(54) var<storage, read_write> group_spills: array<u32>;
@compute @workgroup_size(256)
fn fine_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let pixel_ix = gid.x;
    let pixel_count = config.width * config.height;
    if (pixel_ix >= pixel_count) {
        return;
    }

    let px = pixel_ix % config.width;
    let py = pixel_ix / config.width;
    target_store(px, py, direct_pixel(pixel_ix));
}

fn direct_pixel(pixel_ix: u32) -> u32 {
    let px = pixel_ix % config.width;
    let py = pixel_ix / config.width;
    let sample_x = f32(px) + 0.5;
    let sample_y = f32(py) + 0.5;
    let pixel_x = i32(px);
    let pixel_y = i32(py);
    var dst = config.clear_color;

    for (var draw_ix = 0u; draw_ix < config.draw_count; draw_ix = draw_ix + 1u) {
        if (pixel_x < draw_pixel_x0[draw_ix] || pixel_y < draw_pixel_y0[draw_ix] ||
            pixel_x >= draw_pixel_x1[draw_ix] || pixel_y >= draw_pixel_y1[draw_ix]) {
            continue;
        }

        var coverage = 0.0;
        let flags = draw_flags[draw_ix];
        let sdf_ref = draw_sdf_refs[draw_ix];
        if ((flags & DRAW_FLAG_SOLID_RECT) != 0u) {
            coverage = 1.0;
        } else if ((flags & DRAW_FLAG_HAS_SDF) != 0u && sdf_ref != INVALID_REF) {
            coverage = sdf_coverage_from_encoded(sdf_ref, sample_x, sample_y);
        }
        let alpha = coverage_to_u8(coverage);
        if (alpha != 0u) {
            let color = sample_brush(draw_ix, sample_x, sample_y);
            dst = src_over_premul_u8(dst, scale_premul_u8(color, alpha));
        }
    }

    return dst;
}

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

    var ptcl_ix = tile_range_starts[tile_ix];
    let range_end = tile_range_ends[tile_ix];
    loop {
        if (ptcl_ix >= range_end) {
            break;
        }
        let tag = packed_u8_at(ptcl_ix);
        if (tag == GPU_PTCL_END) {
            break;
        }

        if (tag == GPU_PTCL_COLOR) {
            let color = scale_premul_u8(ptcl_colors[ptcl_ix], clip_mask);
            pixel = src_over_premul_u8(pixel, color);
        } else if (tag == GPU_PTCL_SDF) {
            let draw_ix = ptcl_colors[ptcl_ix];
            let sdf_ref = draw_sdf_refs[draw_ix];
            if (sdf_ref != INVALID_REF) {
                let coverage = sdf_coverage_from_encoded(
                    sdf_ref,
                    f32(global_x) + 0.5,
                    f32(global_y) + 0.5,
                );
                let alpha = combine_alpha(coverage_to_u8(coverage), clip_mask);
                if (alpha != 0u) {
                    let color = sample_brush(draw_ix, f32(global_x) + 0.5, f32(global_y) + 0.5);
                    pixel = src_over_premul_u8(pixel, scale_premul_u8(color, alpha));
                }
            }
        } else if (tag == GPU_PTCL_GLYPH) {
            pixel = composite_glyphs_at(
                pixel,
                ptcl_segment_starts[ptcl_ix],
                ptcl_segment_ends[ptcl_ix],
                ptcl_colors[ptcl_ix],
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
                        clip_mask = clip_spills[stack_ix];
                    }
                }
            } else {
                clip_mask = 255u;
            }
        } else if (tag == GPU_PTCL_BEGIN_SDF_CLIP) {
            let draw_ix = ptcl_colors[ptcl_ix];
            var alpha = 0u;
            let sdf_ref = draw_sdf_refs[draw_ix];
            if (sdf_ref != INVALID_REF) {
                alpha = coverage_to_u8(sdf_coverage_from_encoded(
                    sdf_ref,
                    f32(global_x) + 0.5,
                    f32(global_y) + 0.5,
                ));
            }
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
                        group_kind = group_spills[stack_ix];
                        parent = group_spills[stack_ix + 1u];
                        parent_clip = group_spills[stack_ix + 2u];
                        layer_alpha = group_spills[stack_ix + 3u];
                        payload = group_spills[stack_ix + 4u];
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
                ptcl_backdrops[ptcl_ix],
                ptcl_fill_rules[ptcl_ix],
                ptcl_segment_starts[ptcl_ix],
                ptcl_segment_ends[ptcl_ix],
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
                    group0_payload = ptcl_colors[ptcl_ix];
                    pushed_group = true;
                } else if (group_depth == 1u) {
                    group1_kind = tag;
                    group1_parent_pixel = pixel;
                    group1_parent_clip = clip_mask;
                    group1_layer_alpha = alpha;
                    group1_payload = ptcl_colors[ptcl_ix];
                    pushed_group = true;
                } else {
                    let spill_depth_ix = group_depth - FINE_LOCAL_GROUP_DEPTH;
                    if (spill_depth_ix < config.group_spill_depth) {
                        let stack_ix =
                            ((tile_ix * config.group_spill_depth + spill_depth_ix) *
                                FINE_WORKGROUP_SIZE +
                                local_ix) *
                            FINE_GROUP_SPILL_FIELDS;
                        group_spills[stack_ix] = tag;
                        group_spills[stack_ix + 1u] = pixel;
                        group_spills[stack_ix + 2u] = clip_mask;
                        group_spills[stack_ix + 3u] = alpha;
                        group_spills[stack_ix + 4u] = ptcl_colors[ptcl_ix];
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
                    let draw_ix = ptcl_colors[ptcl_ix];
                    let color = sample_brush(draw_ix, f32(global_x) + 0.5, f32(global_y) + 0.5);
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

fn packed_u8_at(ix: u32) -> u32 {
    let word = ptcl_tags[ix / 4u];
    let shift = (ix % 4u) * 8u;
    return (word >> shift) & 255u;
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
        let glyph_i = glyph_indices[glyph_list_ix];
        let image_id = glyph_image_ids[glyph_i];
        if (image_id != INVALID_REF) {
            let width = glyph_image_width[image_id];
            let height = glyph_image_height[image_id];
            let x0 = glyph_x[glyph_i] + glyph_image_left[image_id];
            let y0 = glyph_y[glyph_i] - glyph_image_top[image_id];
            let local_x = px - x0;
            let local_y = py - y0;
            if (local_x >= 0i && local_y >= 0i && local_x < i32(width) && local_y < i32(height)) {
                let data_ix = glyph_image_data_offsets[image_id] + u32(local_y) * width + u32(local_x);
                let content = glyph_image_content[image_id];
                let data = glyph_image_data[data_ix];
                if (content == GPU_GLYPH_MASK) {
                    let alpha = combine_alpha(data, clip_mask);
                    if (alpha != 0u) {
                        let color = sample_brush(draw_ix, f32(global_x) + 0.5, f32(global_y) + 0.5);
                        pixel = src_over_premul_u8(pixel, scale_premul_u8(color, alpha));
                    }
                } else if (content == GPU_GLYPH_LINEAR_MASK) {
                    let alpha = combine_alpha(data, clip_mask);
                    if (alpha != 0u) {
                        let color = sample_brush(draw_ix, f32(global_x) + 0.5, f32(global_y) + 0.5);
                        pixel = src_over_mask_linear_auto_u8(pixel, color, alpha);
                    }
                } else if (content == GPU_GLYPH_COLOR) {
                    pixel = src_over_premul_u8(pixel, scale_premul_u8(data, clip_mask));
                } else if (content == GPU_GLYPH_LINEAR_COLOR) {
                    pixel = src_over_mask_linear_auto_u8(pixel, data, clip_mask);
                } else if (content == GPU_GLYPH_SUBPIXEL_MASK) {
                    let color = sample_brush(draw_ix, f32(global_x) + 0.5, f32(global_y) + 0.5);
                    pixel = src_over_subpixel_mask_u8(pixel, color, data, clip_mask);
                } else if (content == GPU_GLYPH_LINEAR_SUBPIXEL_MASK) {
                    let color = sample_brush(draw_ix, f32(global_x) + 0.5, f32(global_y) + 0.5);
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
            clip_spills[stack_ix] = mask;
            *depth = d + 1u;
        }
    }
}

fn coverage_to_u8(coverage: f32) -> u32 {
    return u32(clamp(coverage, 0.0, 1.0) * 255.0 + 0.5);
}

fn combine_alpha(a: u32, b: u32) -> u32 {
    return mul_div255(a, b);
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
        let parts = segment_row_parts(
            segment_p0x[segment_ix],
            segment_p0y[segment_ix],
            segment_p1x[segment_ix],
            segment_p1y[segment_ix],
            segment_y_edge[segment_ix],
            y,
        );
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

fn signum_f32(value: f32) -> f32 {
    var out = 1.0;
    if (value < 0.0) {
        out = -1.0;
    }
    return out;
}

fn coverage_to_alpha(value: f32, fill_rule: u32) -> u32 {
    var alpha = min(abs(value), 1.0);
    if (fill_rule == 1u) {
        alpha = abs(value - 2.0 * round(0.5 * value));
    }
    return coverage_to_u8(alpha);
}

fn rgba8_pack(r: u32, g: u32, b: u32, a: u32) -> u32 {
    return r | (g << 8u) | (b << 16u) | (a << 24u);
}

fn target_load(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(target_texture, vec2<i32>(i32(x), i32(y))));
}

fn target_store(x: u32, y: u32, pixel: u32) {
    textureStore(target_texture, vec2<i32>(i32(x), i32(y)), rgba8_to_unorm(pixel));
}

fn unorm_to_rgba8(pixel: vec4<f32>) -> u32 {
    return rgba8_pack(
        u32(clamp(pixel.r * 255.0 + 0.5, 0.0, 255.0)),
        u32(clamp(pixel.g * 255.0 + 0.5, 0.0, 255.0)),
        u32(clamp(pixel.b * 255.0 + 0.5, 0.0, 255.0)),
        u32(clamp(pixel.a * 255.0 + 0.5, 0.0, 255.0)),
    );
}

fn rgba8_to_unorm(pixel: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(pixel & 255u) * (1.0 / 255.0),
        f32((pixel >> 8u) & 255u) * (1.0 / 255.0),
        f32((pixel >> 16u) & 255u) * (1.0 / 255.0),
        f32((pixel >> 24u) & 255u) * (1.0 / 255.0),
    );
}

fn mul_div255(a: u32, b: u32) -> u32 {
    let t = a * b + 128u;
    return (t + (t >> 8u)) >> 8u;
}

fn scale_premul_u8(src: u32, factor: u32) -> u32 {
    if (factor == 0u) {
        return 0u;
    }
    if (factor == 255u) {
        return src;
    }
    return rgba8_pack(
        mul_div255(src & 255u, factor),
        mul_div255((src >> 8u) & 255u, factor),
        mul_div255((src >> 16u) & 255u, factor),
        mul_div255((src >> 24u) & 255u, factor),
    );
}

fn src_over_premul_u8(dst: u32, src: u32) -> u32 {
    let sa = (src >> 24u) & 255u;
    if (sa == 0u) {
        return dst;
    }
    if (sa == 255u) {
        return src;
    }
    let inv = 255u - sa;
    return rgba8_pack(
        (src & 255u) + mul_div255(dst & 255u, inv),
        ((src >> 8u) & 255u) + mul_div255((dst >> 8u) & 255u, inv),
        ((src >> 16u) & 255u) + mul_div255((dst >> 16u) & 255u, inv),
        sa + mul_div255((dst >> 24u) & 255u, inv),
    );
}

fn src_over_subpixel_mask_u8(dst: u32, src: u32, mask_rgb: u32, clip: u32) -> u32 {
    let sa = src >> 24u;
    var out = dst;
    if (sa != 0u && clip != 0u) {
        let mr = combine_alpha(mask_rgb & 255u, clip);
        let mg = combine_alpha((mask_rgb >> 8u) & 255u, clip);
        let mb = combine_alpha((mask_rgb >> 16u) & 255u, clip);
        if (mr != 0u || mg != 0u || mb != 0u) {
            let cr = mul_div255(sa, mr);
            let cg = mul_div255(sa, mg);
            let cb = mul_div255(sa, mb);
            let ca = max(max(cr, cg), cb);
            let r = mul_div255(src & 255u, mr) + mul_div255(dst & 255u, 255u - cr);
            let g = mul_div255((src >> 8u) & 255u, mg) + mul_div255((dst >> 8u) & 255u, 255u - cg);
            let b = mul_div255((src >> 16u) & 255u, mb) + mul_div255((dst >> 16u) & 255u, 255u - cb);
            let a = ca + mul_div255((dst >> 24u) & 255u, 255u - ca);
            out = rgba8_pack(r, g, b, a);
        }
    }
    return out;
}

fn src_over_mask_linear_u8(dst: u32, src: u32, coverage: u32) -> u32 {
    var out = dst;
    if ((src >> 24u) != 0u && coverage != 0u) {
        let coverage_f = f32(coverage) * (1.0 / 255.0);
        let src_a = f32((src >> 24u) & 255u) * (1.0 / 255.0);
        let dst_a = f32((dst >> 24u) & 255u) * (1.0 / 255.0);
        let src_r = linear_premul_from_srgb8(src & 255u, src_a);
        let src_g = linear_premul_from_srgb8((src >> 8u) & 255u, src_a);
        let src_b = linear_premul_from_srgb8((src >> 16u) & 255u, src_a);
        let dst_r = linear_premul_from_srgb8(dst & 255u, dst_a);
        let dst_g = linear_premul_from_srgb8((dst >> 8u) & 255u, dst_a);
        let dst_b = linear_premul_from_srgb8((dst >> 16u) & 255u, dst_a);
        let out_src_a = src_a * coverage_f;
        let out_a = out_src_a + dst_a * (1.0 - out_src_a);
        out = pack_linear_premul_to_srgb8(
            src_r * coverage_f + dst_r * (1.0 - out_src_a),
            src_g * coverage_f + dst_g * (1.0 - out_src_a),
            src_b * coverage_f + dst_b * (1.0 - out_src_a),
            out_a,
        );
    }
    return out;
}

fn src_over_mask_linear_auto_u8(dst: u32, src: u32, coverage_in: u32) -> u32 {
    let coverage = auto_text_coverage(
        dst,
        src,
        coverage_in,
        TEXT_ALPHA_MASK_CHROMA_SCALE,
        TEXT_ALPHA_MASK_LOW_LUMA_CHROMA_REDUCTION * max(TEXT_ALPHA_MASK_CHROMA_SCALE - TEXT_SUBPIXEL_MASK_CHROMA_SCALE, 0.0),
        TEXT_ALPHA_MASK_LOW_LUMA_CONTRAST_LIMIT,
        TEXT_ALPHA_MASK_APPARENT_AXIS_STRENGTH,
        TEXT_ALPHA_MASK_APPARENT_AXIS_LUMA_LIMIT,
        false,
    );
    return src_over_mask_linear_u8(dst, src, coverage);
}

fn src_over_subpixel_mask_linear_u8(dst: u32, src: u32, mask_rgb: u32, clip: u32) -> u32 {
    var out = dst;
    if ((src >> 24u) != 0u && clip != 0u) {
        let mr = f32(combine_alpha(mask_rgb & 255u, clip)) * (1.0 / 255.0);
        let mg = f32(combine_alpha((mask_rgb >> 8u) & 255u, clip)) * (1.0 / 255.0);
        let mb = f32(combine_alpha((mask_rgb >> 16u) & 255u, clip)) * (1.0 / 255.0);
        if (mr != 0.0 || mg != 0.0 || mb != 0.0) {
            let src_a = f32((src >> 24u) & 255u) * (1.0 / 255.0);
            let dst_a = f32((dst >> 24u) & 255u) * (1.0 / 255.0);
            let src_r = linear_premul_from_srgb8(src & 255u, src_a);
            let src_g = linear_premul_from_srgb8((src >> 8u) & 255u, src_a);
            let src_b = linear_premul_from_srgb8((src >> 16u) & 255u, src_a);
            let dst_r = linear_premul_from_srgb8(dst & 255u, dst_a);
            let dst_g = linear_premul_from_srgb8((dst >> 8u) & 255u, dst_a);
            let dst_b = linear_premul_from_srgb8((dst >> 16u) & 255u, dst_a);
            let cr = src_a * mr;
            let cg = src_a * mg;
            let cb = src_a * mb;
            let ca = max(max(cr, cg), cb);
            let out_a = ca + dst_a * (1.0 - ca);
            out = pack_linear_premul_to_srgb8(
                src_r * mr + dst_r * (1.0 - cr),
                src_g * mg + dst_g * (1.0 - cg),
                src_b * mb + dst_b * (1.0 - cb),
                out_a,
            );
        }
    }
    return out;
}

fn src_over_subpixel_mask_linear_auto_u8(dst: u32, src: u32, mask_rgb: u32, clip: u32) -> u32 {
    let chroma_scale = TEXT_SUBPIXEL_MASK_CHROMA_SCALE;
    let low_luma_chroma_reduction = TEXT_SUBPIXEL_MASK_LOW_LUMA_CHROMA_REDUCTION;
    let low_luma_contrast_limit = TEXT_SUBPIXEL_MASK_LOW_LUMA_CONTRAST_LIMIT;
    let r = auto_text_coverage(
        dst,
        src,
        combine_alpha(mask_rgb & 255u, clip),
        chroma_scale,
        low_luma_chroma_reduction,
        low_luma_contrast_limit,
        0.0,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
        true,
    );
    let g = auto_text_coverage(
        dst,
        src,
        combine_alpha((mask_rgb >> 8u) & 255u, clip),
        chroma_scale,
        low_luma_chroma_reduction,
        low_luma_contrast_limit,
        0.0,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
        true,
    );
    let b = auto_text_coverage(
        dst,
        src,
        combine_alpha((mask_rgb >> 16u) & 255u, clip),
        chroma_scale,
        low_luma_chroma_reduction,
        low_luma_contrast_limit,
        0.0,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
        true,
    );
    let compensated = r | (g << 8u) | (b << 16u);
    let corrected = subpixel_axis_corrected_mask(
        dst,
        src,
        compensated,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_STRENGTH,
        TEXT_SUBPIXEL_MASK_APPARENT_AXIS_LUMA_LIMIT,
    );
    return src_over_subpixel_mask_linear_u8(dst, src, corrected, 255u);
}

fn auto_text_coverage(
    dst: u32,
    src: u32,
    coverage_in: u32,
    chroma_scale: f32,
    low_luma_chroma_reduction: f32,
    low_luma_contrast_limit: f32,
    apparent_axis_strength: f32,
    apparent_axis_luma_limit: f32,
    destination_chroma_boost: bool,
) -> u32 {
    var out = coverage_in;
    if (coverage_in != 0u && coverage_in != 255u) {
        let src_alpha = f32((src >> 24u) & 255u) * (1.0 / 255.0);
        let dst_alpha = f32((dst >> 24u) & 255u) * (1.0 / 255.0);
        var src_sr = 0.0;
        var src_sg = 0.0;
        var src_sb = 0.0;
        var dst_sr = 0.0;
        var dst_sg = 0.0;
        var dst_sb = 0.0;
        var src_r = 0.0;
        var src_g = 0.0;
        var src_b = 0.0;
        var dst_r = 0.0;
        var dst_g = 0.0;
        var dst_b = 0.0;
        if (src_alpha > 0.0) {
            let inv_alpha = 1.0 / (src_alpha * 255.0);
            src_sr = clamp(f32(src & 255u) * inv_alpha, 0.0, 1.0);
            src_sg = clamp(f32((src >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            src_sb = clamp(f32((src >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
            src_r = srgb_to_linear(src_sr);
            src_g = srgb_to_linear(src_sg);
            src_b = srgb_to_linear(src_sb);
        }
        if (dst_alpha > 0.0) {
            let inv_alpha = 1.0 / (dst_alpha * 255.0);
            dst_sr = clamp(f32(dst & 255u) * inv_alpha, 0.0, 1.0);
            dst_sg = clamp(f32((dst >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            dst_sb = clamp(f32((dst >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
            dst_r = srgb_to_linear(dst_sr);
            dst_g = srgb_to_linear(dst_sg);
            dst_b = srgb_to_linear(dst_sb);
        }

        let src_luma = 0.2126 * src_r + 0.7152 * src_g + 0.0722 * src_b;
        let dst_luma = 0.2126 * dst_r + 0.7152 * dst_g + 0.0722 * dst_b;
        let src_perceptual_luma = 0.2126 * src_sr + 0.7152 * src_sg + 0.0722 * src_sb;
        let dst_perceptual_luma = 0.2126 * dst_sr + 0.7152 * dst_sg + 0.0722 * dst_sb;
        let src_max = max(max(src_r, src_g), src_b);
        let src_min = min(min(src_r, src_g), src_b);
        let dst_max = max(max(dst_r, dst_g), dst_b);
        let dst_min = min(min(dst_r, dst_g), dst_b);
        let src_chroma = clamp(src_max - src_min, 0.0, 1.0);
        let dst_chroma = clamp(dst_max - dst_min, 0.0, 1.0);
        let channel_contrast = max(max(abs(src_r - dst_r), abs(src_g - dst_g)), abs(src_b - dst_b));
        let luma_contrast = abs(src_luma - dst_luma);
        var low_luma_contrast = 0.0;
        if (low_luma_contrast_limit > 0.0) {
            low_luma_contrast = clamp((low_luma_contrast_limit - luma_contrast) / low_luma_contrast_limit, 0.0, 1.0);
        }
        low_luma_contrast = low_luma_contrast * low_luma_contrast;
        var perceptual_light_on_dark_gate = 0.0;
        if (src_perceptual_luma >= dst_perceptual_luma) {
            perceptual_light_on_dark_gate = 1.0;
        }
        let low_luma_chroma_suppression =
            low_luma_chroma_reduction *
            low_luma_contrast *
            perceptual_light_on_dark_gate *
            channel_contrast *
            clamp((src_chroma + dst_chroma) * 0.5, 0.0, 1.0);
        let src_chroma_dominance = clamp((src_chroma - dst_chroma) * 2.0, 0.0, 1.0);
        var source_chroma_contrast_gate = 0.0;
        if (TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT > 0.0) {
            source_chroma_contrast_gate = clamp((TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT - luma_contrast) / TEXT_SOURCE_CHROMA_COVERAGE_CONTRAST_LIMIT, 0.0, 1.0);
        }
        let source_chroma_coverage_boost =
            TEXT_SOURCE_CHROMA_COVERAGE_BOOST *
            source_chroma_contrast_gate *
            src_chroma_dominance *
            src_chroma *
            channel_contrast;
        if (src_luma < dst_luma) {
            let contrast = clamp(dst_luma - src_luma, 0.0, 1.0);
            let hidden_chroma_contrast = max(channel_contrast - contrast, 0.0);
            let dst_chroma_dominance = clamp((dst_chroma - src_chroma) * 2.0, 0.0, 1.0);
            var dark_on_light_chroma = src_chroma;
            if (destination_chroma_boost) {
                dark_on_light_chroma = max(dark_on_light_chroma, dst_chroma * (1.0 - src_max));
            }
            let curve = clamp(
                contrast * (TEXT_DARK_ON_LIGHT_LUMA_BASE - TEXT_DARK_ON_LIGHT_LUMA_TAPER * dst_luma) +
                    TEXT_DARK_ON_LIGHT_CHROMA_BOOST * chroma_scale * hidden_chroma_contrast * dark_on_light_chroma,
                0.0,
                1.0,
            );
            let exponent = max(
                1.0 - TEXT_DARK_ON_LIGHT_COVERAGE_STRENGTH * curve - source_chroma_coverage_boost +
                    low_luma_chroma_suppression * dst_chroma_dominance,
                0.03,
            );
            let compensated = pow(f32(coverage_in) * (1.0 / 255.0), exponent);
            out = apparent_axis_corrected_coverage(
                compensated,
                src_r,
                src_g,
                src_b,
                dst_r,
                dst_g,
                dst_b,
                src_sr,
                src_sg,
                src_sb,
                dst_sr,
                dst_sg,
                dst_sb,
                src_chroma,
                dst_chroma,
                abs(src_perceptual_luma - dst_perceptual_luma),
                apparent_axis_strength,
                apparent_axis_luma_limit,
            );
        } else {
            let contrast = clamp(src_luma - dst_luma, 0.0, 1.0);
            let black_surface = clamp((TEXT_LIGHT_ON_DARK_BLACK_LUMA_LIMIT - dst_luma) / TEXT_LIGHT_ON_DARK_BLACK_LUMA_LIMIT, 0.0, 1.0);
            var high_luma_chroma = 0.0;
            if (src_max > 0.0) {
                high_luma_chroma = src_chroma * max(src_luma / src_max - TEXT_LIGHT_ON_DARK_HIGH_LUMA_THRESHOLD, 0.0);
            }
            let colored_dark_surface =
                clamp((TEXT_LIGHT_ON_COLORED_DARK_LUMA_LIMIT - dst_luma) / TEXT_LIGHT_ON_COLORED_DARK_LUMA_LIMIT, 0.0, 1.0) *
                clamp(dst_chroma * 4.0, 0.0, 1.0);
            let alpha_mask_chroma_excess = max(chroma_scale - TEXT_SUBPIXEL_MASK_CHROMA_SCALE, 0.0);
            let exponent = max(
                1.0 +
                    black_surface *
                        (TEXT_LIGHT_ON_DARK_COVERAGE_REDUCTION * contrast * src_luma +
                            TEXT_LIGHT_ON_DARK_CHROMA_REDUCTION * chroma_scale * src_chroma * src_max +
                            TEXT_LIGHT_ON_DARK_HIGH_LUMA_CHROMA_REDUCTION * chroma_scale * high_luma_chroma) +
                    TEXT_LIGHT_ON_COLORED_DARK_CHROMA_REDUCTION *
                        alpha_mask_chroma_excess *
                        colored_dark_surface *
                        src_chroma *
                        channel_contrast +
                    low_luma_chroma_suppression -
                    source_chroma_coverage_boost * (1.0 - black_surface),
                0.03,
            );
            let compensated = pow(f32(coverage_in) * (1.0 / 255.0), exponent);
            out = apparent_axis_corrected_coverage(
                compensated,
                src_r,
                src_g,
                src_b,
                dst_r,
                dst_g,
                dst_b,
                src_sr,
                src_sg,
                src_sb,
                dst_sr,
                dst_sg,
                dst_sb,
                src_chroma,
                dst_chroma,
                abs(src_perceptual_luma - dst_perceptual_luma),
                apparent_axis_strength,
                apparent_axis_luma_limit,
            );
        }
    }
    return out;
}

fn apparent_axis_corrected_coverage(
    coverage: f32,
    src_r: f32,
    src_g: f32,
    src_b: f32,
    dst_r: f32,
    dst_g: f32,
    dst_b: f32,
    src_sr: f32,
    src_sg: f32,
    src_sb: f32,
    dst_sr: f32,
    dst_sg: f32,
    dst_sb: f32,
    src_chroma: f32,
    dst_chroma: f32,
    perceptual_luma_contrast: f32,
    strength: f32,
    luma_limit: f32,
) -> u32 {
    var out = u32(clamp(coverage, 0.0, 1.0) * 255.0 + 0.5);
    if (strength > 0.0 && luma_limit > 0.0) {
        let luma_gate = clamp((luma_limit - perceptual_luma_contrast) / luma_limit, 0.0, 1.0);
        let chroma_gate = clamp(max(src_chroma, dst_chroma), 0.0, 1.0);
        if (luma_gate > 0.0 && chroma_gate > 0.0) {
            let axis_r = src_sr - dst_sr;
            let axis_g = src_sg - dst_sg;
            let axis_b = src_sb - dst_sb;
            let denom = axis_r * axis_r + axis_g * axis_g + axis_b * axis_b;
            if (denom > 0.000001) {
                let clamped = clamp(coverage, 0.0, 1.0);
                let mixed_lr = dst_r + (src_r - dst_r) * clamped;
                let mixed_lg = dst_g + (src_g - dst_g) * clamped;
                let mixed_lb = dst_b + (src_b - dst_b) * clamped;
                let mixed_r = linear_to_srgb(mixed_lr);
                let mixed_g = linear_to_srgb(mixed_lg);
                let mixed_b = linear_to_srgb(mixed_lb);
                let projection = clamp(((mixed_r - dst_sr) * axis_r + (mixed_g - dst_sg) * axis_g + (mixed_b - dst_sb) * axis_b) / denom, 0.0, 1.0);
                let derivative = max(
                    (axis_r * linear_to_srgb_derivative(mixed_lr) * (src_r - dst_r) +
                        axis_g * linear_to_srgb_derivative(mixed_lg) * (src_g - dst_g) +
                        axis_b * linear_to_srgb_derivative(mixed_lb) * (src_b - dst_b)) / denom,
                    0.0,
                );
                var correction = 0.0;
                if (derivative > 0.0001) {
                    correction = clamp(strength * luma_gate * chroma_gate, 0.0, 1.0) * (clamped - projection) / clamp(derivative, 0.2, 5.0);
                }
                if (!(correction < 0.0 && perceptual_luma_contrast < luma_limit * 0.05)) {
                    out = u32(clamp(clamped + correction, 0.0, 1.0) * 255.0 + 0.5);
                }
            }
        }
    }
    return out;
}

fn subpixel_axis_corrected_mask(dst: u32, src: u32, mask_rgb: u32, strength: f32, luma_limit: f32) -> u32 {
    var out = mask_rgb;
    if (strength > 0.0 && luma_limit > 0.0) {
        let src_alpha = f32((src >> 24u) & 255u) * (1.0 / 255.0);
        let dst_alpha = f32((dst >> 24u) & 255u) * (1.0 / 255.0);
        var src_sr = 0.0;
        var src_sg = 0.0;
        var src_sb = 0.0;
        var dst_sr = 0.0;
        var dst_sg = 0.0;
        var dst_sb = 0.0;
        if (src_alpha > 0.0) {
            let inv_alpha = 1.0 / (src_alpha * 255.0);
            src_sr = clamp(f32(src & 255u) * inv_alpha, 0.0, 1.0);
            src_sg = clamp(f32((src >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            src_sb = clamp(f32((src >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
        }
        if (dst_alpha > 0.0) {
            let inv_alpha = 1.0 / (dst_alpha * 255.0);
            dst_sr = clamp(f32(dst & 255u) * inv_alpha, 0.0, 1.0);
            dst_sg = clamp(f32((dst >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
            dst_sb = clamp(f32((dst >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
        }
        let src_r = srgb_to_linear(src_sr);
        let src_g = srgb_to_linear(src_sg);
        let src_b = srgb_to_linear(src_sb);
        let dst_r = srgb_to_linear(dst_sr);
        let dst_g = srgb_to_linear(dst_sg);
        let dst_b = srgb_to_linear(dst_sb);
        let src_max = max(max(src_r, src_g), src_b);
        let src_min = min(min(src_r, src_g), src_b);
        let dst_max = max(max(dst_r, dst_g), dst_b);
        let dst_min = min(min(dst_r, dst_g), dst_b);
        let chroma_gate = clamp(max(src_max - src_min, dst_max - dst_min), 0.0, 1.0);
        let src_luma = 0.2126 * src_sr + 0.7152 * src_sg + 0.0722 * src_sb;
        let dst_luma = 0.2126 * dst_sr + 0.7152 * dst_sg + 0.0722 * dst_sb;
        let luma_gate = clamp((luma_limit - abs(src_luma - dst_luma)) / luma_limit, 0.0, 1.0);
        if (luma_gate > 0.0 && chroma_gate > 0.0) {
            let axis_r = src_sr - dst_sr;
            let axis_g = src_sg - dst_sg;
            let axis_b = src_sb - dst_sb;
            let denom = axis_r * axis_r + axis_g * axis_g + axis_b * axis_b;
            if (denom > 0.000001) {
                let rendered = src_over_subpixel_mask_linear_u8(dst, src, mask_rgb, 255u);
                let rendered_alpha = f32((rendered >> 24u) & 255u) * (1.0 / 255.0);
                var px_r = 0.0;
                var px_g = 0.0;
                var px_b = 0.0;
                if (rendered_alpha > 0.0) {
                    let inv_alpha = 1.0 / (rendered_alpha * 255.0);
                    px_r = clamp(f32(rendered & 255u) * inv_alpha, 0.0, 1.0);
                    px_g = clamp(f32((rendered >> 8u) & 255u) * inv_alpha, 0.0, 1.0);
                    px_b = clamp(f32((rendered >> 16u) & 255u) * inv_alpha, 0.0, 1.0);
                }
                let projected = clamp(((px_r - dst_sr) * axis_r + (px_g - dst_sg) * axis_g + (px_b - dst_sb) * axis_b) / denom, 0.0, 1.0);
                let target_coverage =
                    f32((mask_rgb & 255u) + ((mask_rgb >> 8u) & 255u) + ((mask_rgb >> 16u) & 255u)) *
                    (1.0 / 765.0);
                let correction = strength * luma_gate * chroma_gate * max(target_coverage - projected, 0.0);
                let r = u32(clamp(f32(mask_rgb & 255u) * (1.0 / 255.0) + correction, 0.0, 1.0) * 255.0 + 0.5);
                let g = u32(clamp(f32((mask_rgb >> 8u) & 255u) * (1.0 / 255.0) + correction, 0.0, 1.0) * 255.0 + 0.5);
                let b = u32(clamp(f32((mask_rgb >> 16u) & 255u) * (1.0 / 255.0) + correction, 0.0, 1.0) * 255.0 + 0.5);
                out = r | (g << 8u) | (b << 16u);
            }
        }
    }
    return out;
}

fn linear_premul_from_srgb8(value: u32, alpha: f32) -> f32 {
    var out = 0.0;
    if (alpha > 0.0) {
        out = srgb_to_linear((f32(value) * (1.0 / 255.0)) / alpha) * alpha;
    }
    return out;
}

fn pack_linear_premul_to_srgb8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    var out = 0u;
    if (a > 0.0) {
        let alpha = clamp(a, 0.0, 1.0);
        let pr = linear_premul_channel_to_srgb8(r, alpha);
        let pg = linear_premul_channel_to_srgb8(g, alpha);
        let pb = linear_premul_channel_to_srgb8(b, alpha);
        let pa = u32(alpha * 255.0 + 0.5);
        out = pr | (pg << 8u) | (pb << 16u) | (pa << 24u);
    }
    return out;
}

fn linear_premul_channel_to_srgb8(value: f32, alpha: f32) -> u32 {
    return u32(linear_to_srgb(clamp(value / alpha, 0.0, 1.0)) * alpha * 255.0 + 0.5);
}

fn srgb_to_linear(value: f32) -> f32 {
    let v = clamp(value, 0.0, 1.0);
    var out = v / 12.92;
    if (v > 0.04045) {
        out = pow((v + 0.055) / 1.055, 2.4);
    }
    return out;
}

fn linear_to_srgb(value: f32) -> f32 {
    let v = clamp(value, 0.0, 1.0);
    var out = v * 12.92;
    if (v > 0.0031308) {
        out = 1.055 * pow(v, 1.0 / 2.4) - 0.055;
    }
    return out;
}

fn linear_to_srgb_derivative(value: f32) -> f32 {
    let v = clamp(value, 0.0, 1.0);
    var out = 12.92;
    if (v > 0.0031308) {
        out = (1.055 / 2.4) * pow(v, 1.0 / 2.4 - 1.0);
    }
    return out;
}

fn blend_premul_u8(dst: u32, src: u32, mode: u32) -> u32 {
    let mix = mode & 255u;
    let compose = (mode >> 8u) & 255u;
    let inv = 1.0 / 255.0;
    let sr = f32(src & 255u) * inv;
    let sg = f32((src >> 8u) & 255u) * inv;
    let sb = f32((src >> 16u) & 255u) * inv;
    let sa = f32((src >> 24u) & 255u) * inv;
    let dr = f32(dst & 255u) * inv;
    let dg = f32((dst >> 8u) & 255u) * inv;
    let db = f32((dst >> 16u) & 255u) * inv;
    let da = f32((dst >> 24u) & 255u) * inv;

    var out_r = sr + dr * (1.0 - sa);
    var out_g = sg + dg * (1.0 - sa);
    var out_b = sb + db * (1.0 - sa);
    var out_a = sa + da * (1.0 - sa);

    if (mix == 0u && compose == 3u) {
    } else if (mix == 0u && compose == 2u) {
        out_r = dr;
        out_g = dg;
        out_b = db;
        out_a = da;
    } else if (mix == 0u && compose == 0u) {
        out_r = 0.0;
        out_g = 0.0;
        out_b = 0.0;
        out_a = 0.0;
    } else if (mix == 0u && compose == 1u) {
        out_r = sr;
        out_g = sg;
        out_b = sb;
        out_a = sa;
    } else if (mix == 0u) {
        let src_factor = compose_src_factor(compose, sa, da);
        let dst_factor = compose_dst_factor(compose, sa, da);
        out_r = sr * src_factor + dr * dst_factor;
        out_g = sg * src_factor + dg * dst_factor;
        out_b = sb * src_factor + db * dst_factor;
        out_a = sa * src_factor + da * dst_factor;
        if (compose == 13u) {
            out_r = min(out_r, 1.0);
            out_g = min(out_g, 1.0);
            out_b = min(out_b, 1.0);
            out_a = min(out_a, 1.0);
        }
    } else if (compose == 3u && mix == 6u) {
        out_r = color_dodge_premul(sr, dr, sa, da);
        out_g = color_dodge_premul(sg, dg, sa, da);
        out_b = color_dodge_premul(sb, db, sa, da);
        out_a = sa + da * (1.0 - sa);
    } else if (compose == 3u && mix == 7u) {
        out_r = color_burn_premul(sr, dr, sa, da);
        out_g = color_burn_premul(sg, dg, sa, da);
        out_b = color_burn_premul(sb, db, sa, da);
        out_a = sa + da * (1.0 - sa);
    } else {
        let src_alpha = clamp(sa, 0.0, 1.0);
        let dst_alpha = clamp(da, 0.0, 1.0);
        let src_r = unpremul_channel(sr, src_alpha);
        let src_g = unpremul_channel(sg, src_alpha);
        let src_b = unpremul_channel(sb, src_alpha);
        let dst_r = unpremul_channel(dr, dst_alpha);
        let dst_g = unpremul_channel(dg, dst_alpha);
        let dst_b = unpremul_channel(db, dst_alpha);
        let mixed_r = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 0u);
        let mixed_g = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 1u);
        let mixed_b = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 2u);
        let effective_r = src_alpha * ((1.0 - dst_alpha) * src_r + dst_alpha * mixed_r);
        let effective_g = src_alpha * ((1.0 - dst_alpha) * src_g + dst_alpha * mixed_g);
        let effective_b = src_alpha * ((1.0 - dst_alpha) * src_b + dst_alpha * mixed_b);
        let src_factor = compose_src_factor(compose, src_alpha, dst_alpha);
        let dst_factor = compose_dst_factor(compose, src_alpha, dst_alpha);
        out_r = effective_r * src_factor + dr * dst_factor;
        out_g = effective_g * src_factor + dg * dst_factor;
        out_b = effective_b * src_factor + db * dst_factor;
        out_a = src_alpha * src_factor + da * dst_factor;
    }

    return pack_premul_rgba8(out_r, out_g, out_b, out_a);
}

fn compose_src_factor(compose: u32, src_alpha: f32, dst_alpha: f32) -> f32 {
    _ = src_alpha;
    var factor = 1.0;
    if (compose == 0u || compose == 2u || compose == 6u || compose == 8u) {
        factor = 0.0;
    } else if (compose == 4u) {
        factor = 1.0 - dst_alpha;
    } else if (compose == 5u || compose == 9u) {
        factor = dst_alpha;
    } else if (compose == 7u || compose == 10u || compose == 11u) {
        factor = 1.0 - dst_alpha;
    }
    return factor;
}

fn compose_dst_factor(compose: u32, src_alpha: f32, dst_alpha: f32) -> f32 {
    _ = dst_alpha;
    var factor = 1.0 - src_alpha;
    if (compose == 0u || compose == 1u || compose == 5u || compose == 7u) {
        factor = 0.0;
    } else if (compose == 2u || compose == 4u) {
        factor = 1.0;
    } else if (compose == 6u || compose == 10u) {
        factor = src_alpha;
    } else if (compose == 8u || compose == 9u || compose == 11u) {
        factor = 1.0 - src_alpha;
    } else if (compose == 12u || compose == 13u) {
        factor = 1.0;
    }
    return factor;
}

fn unpremul_channel(value: f32, alpha: f32) -> f32 {
    var out = 0.0;
    if (alpha > 0.0) {
        out = value / alpha;
    }
    return out;
}

fn mix_rgb_channel(
    dst_r: f32,
    dst_g: f32,
    dst_b: f32,
    src_r: f32,
    src_g: f32,
    src_b: f32,
    mix: u32,
    channel: u32,
) -> f32 {
    var r = src_r;
    var g = src_g;
    var b = src_b;
    if (mix == 1u) {
        r = dst_r * src_r;
        g = dst_g * src_g;
        b = dst_b * src_b;
    } else if (mix == 2u) {
        r = dst_r + src_r - dst_r * src_r;
        g = dst_g + src_g - dst_g * src_g;
        b = dst_b + src_b - dst_b * src_b;
    } else if (mix == 3u) {
        r = overlay(dst_r, src_r);
        g = overlay(dst_g, src_g);
        b = overlay(dst_b, src_b);
    } else if (mix == 4u) {
        r = min(dst_r, src_r);
        g = min(dst_g, src_g);
        b = min(dst_b, src_b);
    } else if (mix == 5u) {
        r = max(dst_r, src_r);
        g = max(dst_g, src_g);
        b = max(dst_b, src_b);
    } else if (mix == 6u) {
        r = color_dodge(dst_r, src_r);
        g = color_dodge(dst_g, src_g);
        b = color_dodge(dst_b, src_b);
    } else if (mix == 7u) {
        r = color_burn(dst_r, src_r);
        g = color_burn(dst_g, src_g);
        b = color_burn(dst_b, src_b);
    } else if (mix == 8u) {
        r = overlay(src_r, dst_r);
        g = overlay(src_g, dst_g);
        b = overlay(src_b, dst_b);
    } else if (mix == 9u) {
        r = soft_light(dst_r, src_r);
        g = soft_light(dst_g, src_g);
        b = soft_light(dst_b, src_b);
    } else if (mix == 10u) {
        r = abs(dst_r - src_r);
        g = abs(dst_g - src_g);
        b = abs(dst_b - src_b);
    } else if (mix == 11u) {
        r = dst_r + src_r - 2.0 * dst_r * src_r;
        g = dst_g + src_g - 2.0 * dst_g * src_g;
        b = dst_b + src_b - 2.0 * dst_b * src_b;
    } else if (mix == 12u) {
        let sat_dst = sat3(dst_r, dst_g, dst_b);
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        let sr = set_sat_channel(src_r, src_g, src_b, sat_dst, 0u);
        let sg = set_sat_channel(src_r, src_g, src_b, sat_dst, 1u);
        let sb = set_sat_channel(src_r, src_g, src_b, sat_dst, 2u);
        r = set_lum_channel(sr, sg, sb, lum_dst, 0u);
        g = set_lum_channel(sr, sg, sb, lum_dst, 1u);
        b = set_lum_channel(sr, sg, sb, lum_dst, 2u);
    } else if (mix == 13u) {
        let sat_src = sat3(src_r, src_g, src_b);
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        let dr = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 0u);
        let dg = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 1u);
        let db = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 2u);
        r = set_lum_channel(dr, dg, db, lum_dst, 0u);
        g = set_lum_channel(dr, dg, db, lum_dst, 1u);
        b = set_lum_channel(dr, dg, db, lum_dst, 2u);
    } else if (mix == 14u) {
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        r = set_lum_channel(src_r, src_g, src_b, lum_dst, 0u);
        g = set_lum_channel(src_r, src_g, src_b, lum_dst, 1u);
        b = set_lum_channel(src_r, src_g, src_b, lum_dst, 2u);
    } else if (mix == 15u) {
        let lum_src = lum3(src_r, src_g, src_b);
        r = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 0u);
        g = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 1u);
        b = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 2u);
    }

    if (channel == 0u) {
        return r;
    } else if (channel == 1u) {
        return g;
    }
    return b;
}

fn overlay(dst: f32, src: f32) -> f32 {
    if (dst <= 0.5) {
        return 2.0 * dst * src;
    }
    return 1.0 - 2.0 * (1.0 - dst) * (1.0 - src);
}

fn color_dodge(dst: f32, src: f32) -> f32 {
    var out = 1.0;
    if (src < 1.0) {
        out = min(dst / (1.0 - src), 1.0);
    }
    return out;
}

fn color_burn(dst: f32, src: f32) -> f32 {
    var out = 0.0;
    if (src > 0.0) {
        out = 1.0 - min((1.0 - dst) / src, 1.0);
    }
    return out;
}

fn color_dodge_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    var out = src * (1.0 - dst_alpha);
    if (dst > 0.0) {
        if (src >= src_alpha) {
            out = src + dst * (1.0 - src_alpha);
        } else {
            out = src_alpha * min(dst_alpha, (dst * src_alpha) / (src_alpha - src)) +
                src * (1.0 - dst_alpha) +
                dst * (1.0 - src_alpha);
        }
    }
    return out;
}

fn color_burn_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    var out = dst + src * (1.0 - dst_alpha);
    if (dst < dst_alpha) {
        if (src <= 0.0) {
            out = dst * (1.0 - src_alpha);
        } else {
            out = src_alpha * (dst_alpha - min(dst_alpha, ((dst_alpha - dst) * src_alpha) / src)) +
                src * (1.0 - dst_alpha) +
                dst * (1.0 - src_alpha);
        }
    }
    return out;
}

fn soft_light(dst: f32, src: f32) -> f32 {
    var out = dst - (1.0 - 2.0 * src) * dst * (1.0 - dst);
    if (src > 0.5) {
        var d = sqrt(dst);
        if (dst <= 0.25) {
            d = ((16.0 * dst - 12.0) * dst + 4.0) * dst;
        }
        out = dst + (2.0 * src - 1.0) * (d - dst);
    }
    return out;
}

fn lum3(r: f32, g: f32, b: f32) -> f32 {
    return 0.3 * r + 0.59 * g + 0.11 * b;
}

fn sat3(r: f32, g: f32, b: f32) -> f32 {
    return max(max(r, g), b) - min(min(r, g), b);
}

fn set_lum_channel(r: f32, g: f32, b: f32, lum: f32, channel: u32) -> f32 {
    let d = lum - lum3(r, g, b);
    return clip_color_channel(r + d, g + d, b + d, channel);
}

fn clip_color_channel(r: f32, g: f32, b: f32, channel: u32) -> f32 {
    let lum = lum3(r, g, b);
    let min_c = min(min(r, g), b);
    let max_c = max(max(r, g), b);
    var out_r = r;
    var out_g = g;
    var out_b = b;
    if (min_c < 0.0) {
        out_r = lum + (out_r - lum) * lum / (lum - min_c);
        out_g = lum + (out_g - lum) * lum / (lum - min_c);
        out_b = lum + (out_b - lum) * lum / (lum - min_c);
    }
    if (max_c > 1.0) {
        out_r = lum + (out_r - lum) * (1.0 - lum) / (max_c - lum);
        out_g = lum + (out_g - lum) * (1.0 - lum) / (max_c - lum);
        out_b = lum + (out_b - lum) * (1.0 - lum) / (max_c - lum);
    }
    if (channel == 0u) {
        return out_r;
    } else if (channel == 1u) {
        return out_g;
    }
    return out_b;
}

fn set_sat_channel(r: f32, g: f32, b: f32, sat: f32, channel: u32) -> f32 {
    var min_ix = 0u;
    if (r <= g && r <= b) {
    } else if (g <= b) {
        min_ix = 1u;
    } else {
        min_ix = 2u;
    }

    var max_ix = 0u;
    if (r >= g && r >= b) {
    } else if (g >= b) {
        max_ix = 1u;
    } else {
        max_ix = 2u;
    }

    var out_r = 0.0;
    var out_g = 0.0;
    var out_b = 0.0;
    if (min_ix != max_ix) {
        let mid_ix = 3u - min_ix - max_ix;
        let min_v = channel_value(r, g, b, min_ix);
        let mid_v = channel_value(r, g, b, mid_ix);
        let max_v = channel_value(r, g, b, max_ix);
        var new_mid = 0.0;
        var new_max = 0.0;
        if (max_v > min_v) {
            new_mid = (mid_v - min_v) * sat / (max_v - min_v);
            new_max = sat;
        }
        out_r = set_channel_value(out_r, new_mid, mid_ix, 0u);
        out_g = set_channel_value(out_g, new_mid, mid_ix, 1u);
        out_b = set_channel_value(out_b, new_mid, mid_ix, 2u);
        out_r = set_channel_value(out_r, new_max, max_ix, 0u);
        out_g = set_channel_value(out_g, new_max, max_ix, 1u);
        out_b = set_channel_value(out_b, new_max, max_ix, 2u);
    }

    if (channel == 0u) {
        return out_r;
    } else if (channel == 1u) {
        return out_g;
    }
    return out_b;
}

fn channel_value(r: f32, g: f32, b: f32, channel: u32) -> f32 {
    if (channel == 0u) {
        return r;
    } else if (channel == 1u) {
        return g;
    }
    return b;
}

fn set_channel_value(current: f32, value: f32, src_channel: u32, dst_channel: u32) -> f32 {
    if (src_channel == dst_channel) {
        return value;
    }
    return current;
}

fn pack_premul_rgba8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    return u32(clamp(r, 0.0, 1.0) * 255.0 + 0.5) |
        (u32(clamp(g, 0.0, 1.0) * 255.0 + 0.5) << 8u) |
        (u32(clamp(b, 0.0, 1.0) * 255.0 + 0.5) << 16u) |
        (u32(clamp(a, 0.0, 1.0) * 255.0 + 0.5) << 24u);
}

fn sample_brush(brush_index: u32, x: f32, y: f32) -> u32 {
    let data_base = brush_index * GPU_BRUSH_U32_STRIDE;
    let kind = brush_data[data_base];
    let extend = brush_data[data_base + 1u];
    let payload_offset = brush_data[data_base + 2u];
    let payload_len = brush_data[data_base + 3u];
    let base = brush_index * GPU_BRUSH_PARAM_STRIDE;
    var color = brush_data[data_base + 4u];

    if (kind == GPU_BRUSH_LINEAR) {
        let tx = brush_params[base + 4u] * x + brush_params[base + 6u] * y + brush_params[base + 8u];
        let ty = brush_params[base + 5u] * x + brush_params[base + 7u] * y + brush_params[base + 9u];
        let sx = brush_params[base];
        let sy = brush_params[base + 1u];
        let ex = brush_params[base + 2u];
        let ey = brush_params[base + 3u];
        let dx = ex - sx;
        let dy = ey - sy;
        let denominator = dx * dx + dy * dy;
        var t = 0.0;
        if (denominator > 0.00000011920929) {
            t = ((tx - sx) * dx + (ty - sy) * dy) / denominator;
        }
        color = sample_ramp(payload_offset, payload_len, t, extend);
    } else if (kind == GPU_BRUSH_RADIAL) {
        color = sample_radial(x, y, base, extend, payload_offset, payload_len);
    } else if (kind == GPU_BRUSH_SWEEP) {
        let cx = brush_params[base];
        let cy = brush_params[base + 1u];
        let start_angle = brush_params[base + 2u];
        let end_angle = brush_params[base + 3u];
        let span = end_angle - start_angle;
        var t = 0.0;
        if (abs(span) > 0.00000011920929) {
            let tau = 6.2831855;
            var angle = atan2(y - cy, x - cx);
            if (span > 0.0) {
                while (angle < start_angle) {
                    angle = angle + tau;
                }
            } else {
                while (angle > start_angle) {
                    angle = angle - tau;
                }
            }
            t = (angle - start_angle) / span;
        }
        color = sample_ramp(payload_offset, payload_len, t, extend);
    } else if (kind == GPU_BRUSH_FOUR_CORNER) {
        color = sample_four_corner(x, y, base, payload_offset);
    } else if (kind == GPU_BRUSH_PATTERN) {
        color = sample_pattern(
            x,
            y,
            base,
            payload_offset,
            payload_len,
            brush_data[data_base + 5u],
            brush_data[data_base + 6u],
            brush_data[data_base + 7u],
            extend,
            brush_data[data_base + 8u],
        );
    }

    return color;
}

fn sample_radial(x: f32, y: f32, base: u32, extend: u32, payload_offset: u32, payload_len: u32) -> u32 {
    let tx = brush_params[base + 6u] * x + brush_params[base + 8u] * y + brush_params[base + 10u];
    let ty = brush_params[base + 7u] * x + brush_params[base + 9u] * y + brush_params[base + 11u];
    let sx = brush_params[base];
    let sy = brush_params[base + 1u];
    let ex = brush_params[base + 2u];
    let ey = brush_params[base + 3u];
    let start_radius = brush_params[base + 4u];
    let end_radius = brush_params[base + 5u];
    let qx = tx - sx;
    let qy = ty - sy;
    let dcx = ex - sx;
    let dcy = ey - sy;
    let dr = end_radius - start_radius;
    let a = dcx * dcx + dcy * dcy - dr * dr;
    let b = -2.0 * (qx * dcx + qy * dcy + start_radius * dr);
    let c = qx * qx + qy * qy - start_radius * start_radius;
    var has_t = false;
    var t = 0.0;

    if (abs(a) <= 0.000001) {
        if (abs(b) > 0.000001) {
            let candidate = -c / b;
            if (start_radius + candidate * dr >= 0.0) {
                has_t = true;
                t = candidate;
            }
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if (discriminant >= 0.0) {
            let root = sqrt(discriminant);
            let t0 = (-b - root) / (2.0 * a);
            let t1 = (-b + root) / (2.0 * a);
            let valid0 = start_radius + t0 * dr >= 0.0;
            let valid1 = start_radius + t1 * dr >= 0.0;
            if (valid0) {
                has_t = true;
                if (valid1) {
                    t = max(t0, t1);
                } else {
                    t = t0;
                }
            } else if (valid1) {
                has_t = true;
                t = t1;
            }
        }
    }

    var color = 0u;
    if (has_t) {
        color = sample_ramp(payload_offset, payload_len, t, extend);
    }
    return color;
}

fn sample_four_corner(x: f32, y: f32, base: u32, payload_offset: u32) -> u32 {
    let x0 = brush_params[base];
    let y0 = brush_params[base + 1u];
    let x1 = brush_params[base + 2u];
    let y1 = brush_params[base + 3u];
    let width = x1 - x0;
    let height = y1 - y0;
    var u = 0.0;
    var v = 0.0;
    if (abs(width) > 0.00000011920929) {
        u = clamp((x - x0) / width, 0.0, 1.0);
    }
    if (abs(height) > 0.00000011920929) {
        v = clamp((y - y0) / height, 0.0, 1.0);
    }
    let tl = brush_payloads[payload_offset];
    let tr = brush_payloads[payload_offset + 1u];
    let br = brush_payloads[payload_offset + 2u];
    let bl = brush_payloads[payload_offset + 3u];
    let top = lerp_premul_u8(tl, tr, u);
    let bottom = lerp_premul_u8(bl, br, u);
    return lerp_premul_u8(top, bottom, v);
}

fn sample_pattern(
    x: f32,
    y: f32,
    base: u32,
    payload_offset: u32,
    payload_len: u32,
    width: u32,
    height: u32,
    opacity: u32,
    extend: u32,
    sampling: u32,
) -> u32 {
    var color = 0u;
    if (payload_len > 0u && width > 0u && height > 0u) {
        let tx = brush_params[base] * x + brush_params[base + 2u] * y + brush_params[base + 4u];
        let ty = brush_params[base + 1u] * x + brush_params[base + 3u] * y + brush_params[base + 5u];
        if (sampling == GPU_PATTERN_BILINEAR) {
            let sx = tx - 0.5;
            let sy = ty - 0.5;
            let x0f = floor(sx);
            let y0f = floor(sy);
            let fx = sx - x0f;
            let fy = sy - y0f;
            let x0 = i32(x0f);
            let y0 = i32(y0f);
            let tl = pattern_pixel(payload_offset, payload_len, width, height, extend, x0, y0);
            let tr = pattern_pixel(payload_offset, payload_len, width, height, extend, x0 + 1, y0);
            let bl = pattern_pixel(payload_offset, payload_len, width, height, extend, x0, y0 + 1);
            let br = pattern_pixel(payload_offset, payload_len, width, height, extend, x0 + 1, y0 + 1);
            color = lerp_premul_u8(lerp_premul_u8(tl, tr, fx), lerp_premul_u8(bl, br, fx), fy);
        } else {
            color = pattern_pixel(payload_offset, payload_len, width, height, extend, i32(floor(tx)), i32(floor(ty)));
        }
        color = scale_premul_u8(color, opacity);
    }
    return color;
}

fn pattern_pixel(payload_offset: u32, payload_len: u32, width: u32, height: u32, extend: u32, x: i32, y: i32) -> u32 {
    let local_x = extend_coord_i32(x, width, extend);
    let local_y = extend_coord_i32(y, height, extend);
    let local_ix = min(local_y * width + local_x, payload_len - 1u);
    return brush_payloads[payload_offset + local_ix];
}

fn sample_ramp(payload_offset: u32, payload_len: u32, t: f32, extend: u32) -> u32 {
    var color = 0u;
    if (payload_len > 0u) {
        let last = payload_len - 1u;
        let position = apply_extend(t, extend) * f32(last);
        let left_ix = u32(floor(position));
        let right_ix = min(left_ix + 1u, last);
        let frac = position - f32(left_ix);
        let left = brush_payloads[payload_offset + left_ix];
        let right = brush_payloads[payload_offset + right_ix];
        if (frac <= 0.00000011920929 || left_ix == right_ix) {
            color = left;
        } else {
            color = lerp_premul_u8(left, right, frac);
        }
    }
    return color;
}

fn apply_extend(t: f32, extend: u32) -> f32 {
    var out = clamp(t, 0.0, 1.0);
    if (extend == GPU_EXTEND_REPEAT) {
        out = rem_euclid_f32(t, 1.0);
    } else if (extend == GPU_EXTEND_REFLECT) {
        let value = rem_euclid_f32(t, 2.0);
        if (value <= 1.0) {
            out = value;
        } else {
            out = 2.0 - value;
        }
    }
    return out;
}

fn repeat_coord_i32(value: i32, size: u32) -> u32 {
    let size_i = i32(size);
    var out = value % size_i;
    if (out < 0) {
        out = out + size_i;
    }
    return u32(out);
}

fn extend_coord_i32(value: i32, size: u32, extend: u32) -> u32 {
    let max_coord = i32(size) - 1;
    var clamped = value;
    if (clamped < 0) {
        clamped = 0;
    }
    if (clamped > max_coord) {
        clamped = max_coord;
    }
    var out = u32(clamped);
    if (extend == GPU_EXTEND_REPEAT) {
        out = repeat_coord_i32(value, size);
    } else if (extend == GPU_EXTEND_REFLECT) {
        out = reflect_coord_i32(value, size);
    }
    return out;
}

fn reflect_coord_i32(value: i32, size: u32) -> u32 {
    var out = 0u;
    if (size > 1u) {
        let size_i = i32(size);
        let period = size_i * 2;
        var coord = value % period;
        if (coord < 0) {
            coord = coord + period;
        }
        if (coord < size_i) {
            out = u32(coord);
        } else {
            out = u32(period - coord - 1);
        }
    }
    return out;
}

fn lerp_premul_u8(a: u32, b: u32, t: f32) -> u32 {
    let inv = 1.0 / 255.0;
    let ar = f32(a & 255u) * inv;
    let ag = f32((a >> 8u) & 255u) * inv;
    let ab = f32((a >> 16u) & 255u) * inv;
    let aa = f32((a >> 24u) & 255u) * inv;
    let br = f32(b & 255u) * inv;
    let bg = f32((b >> 8u) & 255u) * inv;
    let bb = f32((b >> 16u) & 255u) * inv;
    let ba = f32((b >> 24u) & 255u) * inv;
    return rgba8_pack(
        u32(clamp(ar + (br - ar) * t, 0.0, 1.0) * 255.0 + 0.5),
        u32(clamp(ag + (bg - ag) * t, 0.0, 1.0) * 255.0 + 0.5),
        u32(clamp(ab + (bb - ab) * t, 0.0, 1.0) * 255.0 + 0.5),
        u32(clamp(aa + (ba - aa) * t, 0.0, 1.0) * 255.0 + 0.5),
    );
}

fn sdf_coverage_from_encoded(sdf_ref: u32, x: f32, y: f32) -> f32 {
    let x0 = sdf_x0[sdf_ref];
    let y0 = sdf_y0[sdf_ref];
    let x1 = sdf_x1[sdf_ref];
    let y1 = sdf_y1[sdf_ref];
    let r0 = sdf_r0[sdf_ref];
    let r1 = sdf_r1[sdf_ref];
    let r2 = sdf_r2[sdf_ref];
    let r3 = sdf_r3[sdf_ref];
    let stroke_top = sdf_stroke_top[sdf_ref];
    let stroke_right = sdf_stroke_right[sdf_ref];
    let stroke_bottom = sdf_stroke_bottom[sdf_ref];
    let stroke_left = sdf_stroke_left[sdf_ref];
    let shadow_offset_x = sdf_shadow_offset_x[sdf_ref];
    let shadow_offset_y = sdf_shadow_offset_y[sdf_ref];
    let shadow_expand = sdf_shadow_expand[sdf_ref];
    let shadow_intensity = sdf_shadow_intensity[sdf_ref];
    let kind = sdf_kinds[sdf_ref];

    if (kind == GPU_SDF_RECT) {
        return sdf_coverage_from_dist(rect_sdf_distance(x, y, x0, y0, x1, y1, r0, r1, r2, r3));
    }
    if (kind == GPU_SDF_RECT_STROKE) {
        let half_top = max(stroke_top, 0.0);
        let half_right = max(stroke_right, 0.0);
        let half_bottom = max(stroke_bottom, 0.0);
        let half_left = max(stroke_left, 0.0);
        let rx0 = min(x0, x1);
        let ry0 = min(y0, y1);
        let rx1 = max(x0, x1);
        let ry1 = max(y0, y1);
        let outer = sdf_coverage_from_dist(rect_sdf_distance(
            x,
            y,
            rx0 - half_left,
            ry0 - half_top,
            rx1 + half_right,
            ry1 + half_bottom,
            r0 + max(half_top, half_left),
            r1 + max(half_top, half_right),
            r2 + max(half_bottom, half_left),
            r3 + max(half_bottom, half_right),
        ));
        let inner_x0 = rx0 + half_left;
        let inner_y0 = ry0 + half_top;
        let inner_x1 = rx1 - half_right;
        let inner_y1 = ry1 - half_bottom;
        var inner = 0.0;
        if (inner_x0 < inner_x1 && inner_y0 < inner_y1) {
            inner = sdf_coverage_from_dist(rect_sdf_distance(
                x,
                y,
                inner_x0,
                inner_y0,
                inner_x1,
                inner_y1,
                max(r0 - max(half_top, half_left), 0.0),
                max(r1 - max(half_top, half_right), 0.0),
                max(r2 - max(half_bottom, half_left), 0.0),
                max(r3 - max(half_bottom, half_right), 0.0),
            ));
        }
        return clamp(outer - inner, 0.0, 1.0);
    }
    if (kind == GPU_SDF_RECT_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            rect_sdf_distance(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
                r3,
            ),
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_CIRCLE) {
        return sdf_coverage_from_dist(circle_sdf_distance(x, y, x0, y0, x1));
    }
    if (kind == GPU_SDF_CIRCLE_STROKE) {
        let half = max(stroke_top, 0.0);
        let radius = max(x1, 0.0);
        let outer = sdf_coverage_from_dist(circle_sdf_distance(x, y, x0, y0, radius + half));
        var inner = 0.0;
        if (radius > half) {
            inner = sdf_coverage_from_dist(circle_sdf_distance(x, y, x0, y0, radius - half));
        }
        return clamp(outer - inner, 0.0, 1.0);
    }
    if (kind == GPU_SDF_CIRCLE_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            circle_sdf_distance(x - shadow_offset_x, y - shadow_offset_y, x0, y0, x1),
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_ARC) {
        return sdf_coverage_from_dist(arc_sdf_distance(x, y, x0, y0, x1, y1, r0, r1, r2));
    }
    if (kind == GPU_SDF_ARC_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            arc_sdf_distance(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
            ),
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_CANDLESTICK) {
        return candlestick_sdf_coverage(x, y, x0, y0, x1, y1, r0, r1, r2);
    }
    if (kind == GPU_SDF_LINE) {
        return sdf_coverage_from_dist(line_sdf_distance(x, y, x0, y0, x1, y1, r0, r1));
    }
    if (kind == GPU_SDF_DASH_LINE) {
        return sdf_coverage_from_dist(dash_line_sdf_distance(x, y, x0, y0, x1, y1, r0, r1, r2, r3, stroke_top));
    }
    if (kind == GPU_SDF_LINE_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            line_sdf_distance(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
            ),
            shadow_expand,
            shadow_intensity,
        );
    }
    return 0.0;
}

fn dash_line_sdf_distance(
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    ex: f32,
    ey: f32,
    width: f32,
    cap: f32,
    dash_length_raw: f32,
    gap_length_raw: f32,
    dash_offset: f32,
) -> f32 {
    let dash_length = max(dash_length_raw, 0.0);
    let gap_length = max(gap_length_raw, 0.0);
    var dist = line_sdf_distance(x, y, sx, sy, ex, ey, width, cap);
    if (dash_length > 0.000001 && gap_length > 0.000001) {
        let half = max(width, 0.0) * 0.5;
        let dx = ex - sx;
        let dy = ey - sy;
        let len = sqrt(dx * dx + dy * dy);
        if (len > 0.000001) {
            let ux = dx / len;
            let uy = dy / len;
            let px = x - sx;
            let py = y - sy;
            let axis = px * ux + py * uy;
            let normal = -px * uy + py * ux;
            let cycle = dash_length + gap_length;
            let offset = rem_euclid_f32(dash_offset, cycle);
            let base = floor((axis + offset) / cycle);
            dist = min(
                min(
                    dash_line_segment_distance(axis, normal, len, half, cap, dash_length, cycle, offset, base - 1.0),
                    dash_line_segment_distance(axis, normal, len, half, cap, dash_length, cycle, offset, base),
                ),
                dash_line_segment_distance(axis, normal, len, half, cap, dash_length, cycle, offset, base + 1.0),
            );
        }
    }
    return dist;
}

fn dash_line_segment_distance(
    axis: f32,
    normal: f32,
    len: f32,
    half: f32,
    cap: f32,
    dash_length: f32,
    cycle: f32,
    offset: f32,
    dash_ix: f32,
) -> f32 {
    let dash_start = dash_ix * cycle - offset;
    let dash_end = dash_start + dash_length;
    var dist = 1000000.0;
    if (dash_end > 0.0 && dash_start < len) {
        let start = max(dash_start, 0.0);
        let end = min(dash_end, len);
        if (end > start) {
            dist = line_segment_sdf_distance(axis, normal, start, end, half, cap);
        }
    }
    return dist;
}

fn line_segment_sdf_distance(axis: f32, normal: f32, start: f32, end: f32, half: f32, cap: f32) -> f32 {
    let nearest = clamp(axis, start, end);
    var dist = sqrt((axis - nearest) * (axis - nearest) + normal * normal) - half;
    if (cap < 0.5) {
        dist = local_line_rect_distance(axis, normal, start, end, half);
    } else if (cap < 1.5) {
        dist = local_line_rect_distance(axis, normal, start - half, end + half, half);
    }
    return dist;
}

fn candlestick_sdf_coverage(
    x: f32,
    y: f32,
    center_x: f32,
    high_y: f32,
    low_y: f32,
    body_top_y: f32,
    body_bottom_y: f32,
    body_width: f32,
    wick_width: f32,
) -> f32 {
    let wick_half_width = max(wick_width, 1.0) * 0.5;
    let wick = sdf_coverage_from_dist(rect_sdf_distance(
        x,
        y,
        center_x - wick_half_width,
        min(high_y, low_y),
        center_x + wick_half_width,
        max(high_y, low_y),
        0.0,
        0.0,
        0.0,
        0.0,
    ));
    let half_width = max(body_width, 1.0) * 0.5;
    var body_y0 = min(body_top_y, body_bottom_y);
    var body_y1 = max(body_top_y, body_bottom_y);
    if (body_y0 == body_y1) {
        body_y0 = body_y0 - 0.5;
        body_y1 = body_y1 + 0.5;
    }
    let body = sdf_coverage_from_dist(rect_sdf_distance(
        x,
        y,
        center_x - half_width,
        body_y0,
        center_x + half_width,
        body_y1,
        0.0,
        0.0,
        0.0,
        0.0,
    ));
    return max(wick, body);
}

fn line_sdf_distance(x: f32, y: f32, sx: f32, sy: f32, ex: f32, ey: f32, width: f32, cap: f32) -> f32 {
    let half = max(width, 0.0) * 0.5;
    let dx = ex - sx;
    let dy = ey - sy;
    let len = sqrt(dx * dx + dy * dy);
    var dist = 1000000.0;
    if (len <= 0.000001) {
        if (cap >= 0.5) {
            if (cap > 1.5) {
                dist = sqrt((x - sx) * (x - sx) + (y - sy) * (y - sy)) - half;
            } else {
                dist = local_line_rect_distance(0.0, 0.0, -half, half, half);
            }
        }
    } else {
        let ux = dx / len;
        let uy = dy / len;
        let px = x - sx;
        let py = y - sy;
        let axis = px * ux + py * uy;
        let normal = -px * uy + py * ux;
        if (cap < 0.5) {
            dist = local_line_rect_distance(axis, normal, 0.0, len, half);
        } else if (cap < 1.5) {
            dist = local_line_rect_distance(axis, normal, -half, len + half, half);
        } else {
            let nearest = clamp(axis, 0.0, len);
            dist = sqrt((axis - nearest) * (axis - nearest) + normal * normal) - half;
        }
    }
    return dist;
}

fn arc_sdf_distance(
    x: f32,
    y: f32,
    cx: f32,
    cy: f32,
    radius_raw: f32,
    width_raw: f32,
    start_angle: f32,
    sweep_angle: f32,
    cap: f32,
) -> f32 {
    let radius = max(radius_raw, 0.0);
    let width = max(width_raw, 0.0);
    var dist = 1000000.0;
    if (radius > 0.000001 && width > 0.000001 && abs(sweep_angle) > 0.000001) {
        let vx = x - cx;
        let vy = y - cy;
        let len = sqrt(vx * vx + vy * vy);
        let half = width * 0.5;
        if (abs(sweep_angle) >= 6.2830853) {
            dist = abs(len - radius) - half;
        } else {
            let body = arc_butt_sdf_distance(vx, vy, len, radius, half, start_angle, sweep_angle);
            dist = body;
            if (cap > 1.5) {
                dist = min(
                    min(dist, arc_endpoint_distance(vx, vy, radius, start_angle) - half),
                    arc_endpoint_distance(vx, vy, radius, start_angle + sweep_angle) - half,
                );
            } else if (cap >= 0.5) {
                dist = min(
                    min(dist, arc_square_cap_distance(vx, vy, radius, start_angle, sweep_angle, -half, 0.0, half)),
                    arc_square_cap_distance(vx, vy, radius, start_angle + sweep_angle, sweep_angle, 0.0, half, half),
                );
            }
        }
    }
    return dist;
}

fn arc_butt_sdf_distance(vx: f32, vy: f32, len: f32, radius: f32, half: f32, start_angle: f32, sweep_angle: f32) -> f32 {
    if (len <= 0.000001) {
        return min(
            arc_endpoint_distance(vx, vy, radius, start_angle),
            arc_endpoint_distance(vx, vy, radius, start_angle + sweep_angle),
        ) - half;
    }
    let angle = atan2(vy, vx);
    let radial = abs(len - radius) - half;
    if (arc_angle_in_sweep(angle, start_angle, sweep_angle)) {
        return radial;
    }
    return min(
        arc_cap_segment_distance(vx, vy, radius, start_angle, half),
        arc_cap_segment_distance(vx, vy, radius, start_angle + sweep_angle, half),
    );
}

fn arc_angle_in_sweep(angle: f32, start_angle: f32, sweep_angle: f32) -> bool {
    let tau = 6.2831855;
    let eps = 0.000001;
    if (sweep_angle >= 0.0) {
        return rem_euclid_f32(angle - start_angle, tau) <= sweep_angle + eps;
    }
    return rem_euclid_f32(start_angle - angle, tau) <= -sweep_angle + eps;
}

fn arc_endpoint_distance(vx: f32, vy: f32, radius: f32, angle: f32) -> f32 {
    let ex = radius * cos(angle);
    let ey = radius * sin(angle);
    return sqrt((vx - ex) * (vx - ex) + (vy - ey) * (vy - ey));
}

fn arc_cap_segment_distance(vx: f32, vy: f32, radius: f32, angle: f32, half: f32) -> f32 {
    let inner_radius = max(radius - half, 0.0);
    let outer_radius = radius + half;
    let co = cos(angle);
    let si = sin(angle);
    return distance_to_segment(
        vx,
        vy,
        inner_radius * co,
        inner_radius * si,
        outer_radius * co,
        outer_radius * si,
    );
}

fn arc_square_cap_distance(
    vx: f32,
    vy: f32,
    radius: f32,
    angle: f32,
    sweep_angle: f32,
    x0: f32,
    x1: f32,
    half: f32,
) -> f32 {
    var dir = 1.0;
    if (sweep_angle < 0.0) {
        dir = -1.0;
    }
    let co = cos(angle);
    let si = sin(angle);
    let ex = radius * co;
    let ey = radius * si;
    let tangent_x = -si * dir;
    let tangent_y = co * dir;
    let px = vx - ex;
    let py = vy - ey;
    let local_x = px * tangent_x + py * tangent_y;
    let local_y = -px * tangent_y + py * tangent_x;
    return local_line_rect_distance(local_x, local_y, x0, x1, half);
}

fn distance_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    var dist = sqrt((px - ax) * (px - ax) + (py - ay) * (py - ay));
    if (len2 > 0.000001) {
        let t = clamp(((px - ax) * dx + (py - ay) * dy) / len2, 0.0, 1.0);
        let nx = ax + dx * t;
        let ny = ay + dy * t;
        dist = sqrt((px - nx) * (px - nx) + (py - ny) * (py - ny));
    }
    return dist;
}

fn rem_euclid_f32(value: f32, modulus: f32) -> f32 {
    return value - floor(value / modulus) * modulus;
}

fn local_line_rect_distance(axis: f32, normal: f32, x0: f32, x1: f32, half_height: f32) -> f32 {
    let center = (x0 + x1) * 0.5;
    let half_width = (x1 - x0) * 0.5;
    let dx = abs(axis - center) - half_width;
    let dy = abs(normal) - half_height;
    return sqrt(max(dx, 0.0) * max(dx, 0.0) + max(dy, 0.0) * max(dy, 0.0)) + min(max(dx, dy), 0.0);
}

fn sdf_coverage_from_dist(dist: f32) -> f32 {
    return clamp(0.5 - dist, 0.0, 1.0);
}

fn sdf_shadow_coverage_from_dist(dist: f32, expand: f32, intensity: f32) -> f32 {
    let clamped_intensity = clamp(intensity, 0.0, 1.0);
    var coverage = sdf_coverage_from_dist(dist) * clamped_intensity;
    if (expand > 0.0) {
        coverage = exp(-max(dist, 0.0) / expand) * clamped_intensity;
    }
    return clamp(coverage, 0.0, 1.0);
}

fn circle_sdf_distance(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> f32 {
    let dx = x - cx;
    let dy = y - cy;
    return sqrt(dx * dx + dy * dy) - radius;
}

fn rect_sdf_distance(
    x: f32,
    y: f32,
    x0_raw: f32,
    y0_raw: f32,
    x1_raw: f32,
    y1_raw: f32,
    top_left: f32,
    top_right: f32,
    bottom_left: f32,
    bottom_right: f32,
) -> f32 {
    let x0 = min(x0_raw, x1_raw);
    let y0 = min(y0_raw, y1_raw);
    let x1 = max(x0_raw, x1_raw);
    let y1 = max(y0_raw, y1_raw);
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = (x1 - x0) * 0.5;
    let hy = (y1 - y0) * 0.5;
    let px = x - cx;
    let py = y - cy;
    var radius = top_left;
    if (px >= 0.0) {
        if (py <= 0.0) {
            radius = top_right;
        } else {
            radius = bottom_right;
        }
    } else if (py > 0.0) {
        radius = bottom_left;
    }
    let r = max(min(min(radius, hx), hy), 0.0);
    let ax = abs(px);
    let ay = abs(py);

    if (r <= 0.0) {
        let dx = ax - hx;
        let dy = ay - hy;
        return sqrt(max(dx, 0.0) * max(dx, 0.0) + max(dy, 0.0) * max(dy, 0.0)) + min(max(dx, dy), 0.0);
    }

    let qx = ax - hx + r;
    let qy = ay - hy + r;
    return min(max(qx, qy), 0.0) + sqrt(max(qx, 0.0) * max(qx, 0.0) + max(qy, 0.0) * max(qy, 0.0)) - r;
}
