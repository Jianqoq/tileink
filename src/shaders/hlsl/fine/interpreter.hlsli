#ifndef TILEINK_HLSL_FINE_INTERPRETER_INCLUDED
#define TILEINK_HLSL_FINE_INTERPRETER_INCLUDED
#include "inputs.hlsli"
#include "records.hlsli"
#include "constants.hlsli"
#include "../constants.hlsli"
#include "../shared/texture_table_constants.hlsli"
#include "../shared/particle_tags.hlsli"
#include "../shared/draw_tags.hlsli"
#include "../shared/pixel.hlsli"
#include "pixel.hlsli"
#include "draw.hlsli"
#include "../shared/coverage.hlsli"
#include "../shared/brush/data.hlsli"
#include "../shared/blend.hlsli"
#include "text/auto.hlsli"
#include "clip.hlsli"
#include "glyph.hlsli"
#include "group.hlsli"

float4 tile_pixel(FineInputs input, Texture2D<float4> images[NATIVE_TEXTURE_TABLE_CAPACITY], uint tile_ix, uint local_ix) {
    uint local_x = local_ix % TILE_SIZE;
    uint local_y = local_ix / TILE_SIZE;
    uint tile_x = tile_ix % input.config.tiles_width;
    uint tile_y = tile_ix / input.config.tiles_width;
    uint global_x = tile_x * TILE_SIZE + local_x;
    uint global_y = tile_y * TILE_SIZE + local_y;
    // Fine can see many particles for the same pixel; keeping the accumulator in
    // premultiplied float avoids decode/pack work on the hot src-over paths.
    float4 pixel = rgba8_to_unorm(input.config.clear_color);
    if (input.config.load_target != 0u) {
        pixel = input.target[uint2(global_x,global_y)];
    }
    uint clip_mask = 255u;
    uint clip_depth = 0u;
    uint clip_stack0 = 255u;
    uint clip_stack1 = 255u;
    uint clip_stack2 = 255u;
    uint clip_stack3 = 255u;
    uint group_depth = 0u;
    FineGroup group0 = (FineGroup)0;
    FineGroup group1 = (FineGroup)0;

    FineTile tile = coarse_load_tile(input, tile_ix);
    uint ptcl_ix = tile.ptcl_start;
    uint range_end = tile.ptcl_end;
    while (true) {
        if (ptcl_ix >= range_end) {
            break;
        }
        FineParticle ptcl = coarse_load_ptcl(input, ptcl_ix);
        uint tag = ptcl.tag;
        if (tag == PTCL_END) {
            break;
        }

        if (tag == PTCL_COLOR) {
            if (clip_mask != 0u) {
                if (clip_mask == 255u && premul_u8_is_opaque(ptcl.color)) {
                    pixel = rgba8_to_unorm(ptcl.color);
                } else {
                    pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(ptcl.color, clip_mask));
                }
            }
        } else if (tag == PTCL_IMAGE) {
            if (clip_mask != 0u) {
                FineDraw draw = load_fine_draw(input.draws,ptcl.color);
                uint color = sample_draw_brush(input, images, draw, float(global_x) + 0.5, float(global_y) + 0.5);
                if (clip_mask == 255u && premul_u8_is_opaque(color)) {
                    pixel = rgba8_to_unorm(color);
                } else {
                    pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(color, clip_mask));
                }
            }
        } else if (tag == PTCL_SDF) {
            if (clip_mask != 0u) {
                uint draw_ix = ptcl.color;
                FineDraw draw = load_fine_draw(input.draws,draw_ix);
                float coverage = sdf_coverage_from_draw(input,
                    draw,
                    float(global_x) + 0.5,
                    float(global_y) + 0.5);
                uint alpha = combine_alpha(coverage_to_u8(coverage), clip_mask);
                if (alpha != 0u) {
                    uint color = sample_draw_brush(input, images, draw, float(global_x) + 0.5, float(global_y) + 0.5);
                    if (alpha == 255u && premul_u8_is_opaque(color)) {
                        pixel = rgba8_to_unorm(color);
                    } else {
                        pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(color, alpha));
                    }
                }
            }
        } else if (tag == PTCL_GLYPH) {
            FineDraw draw = load_fine_draw(input.draws,ptcl.color);
            // Coarse bounds select whole 16px tiles; enforce the exact glyph draw domain here so
            // an edge tile cannot leak past a draw-local text clip.
            if (clip_mask != 0u && pixel_in_draw_bounds(draw, global_x, global_y)) {
                pixel = composite_glyphs_at(input, images,
                    pixel,
                    ptcl.segment_start,
                    ptcl.segment_end,
                    draw,
                    global_x,
                    global_y,
                    clip_mask);
            }
        } else if (tag == PTCL_END_CLIP) {
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
                    uint spill_depth_ix = clip_depth - FINE_LOCAL_CLIP_DEPTH;
                    if (spill_depth_ix < input.config.clip_spill_depth) {
                        uint stack_ix =
                            (tile_ix * input.config.clip_spill_depth + spill_depth_ix) *
                            FINE_WORKGROUP_SIZE +
                            local_ix;
                        clip_mask = input.spills.Load((stack_ix)*4u);
                    }
                }
            } else {
                clip_mask = 255u;
            }
        } else if (tag == PTCL_BEGIN_SDF_CLIP) {
            uint draw_ix = ptcl.color;
            uint parent_clip = clip_mask;
            uint alpha = 0u;
            if (parent_clip != 0u) {
                alpha = coverage_to_u8(sdf_coverage_from_draw(input,
                    load_fine_draw(input.draws,draw_ix),
                    float(global_x) + 0.5,
                    float(global_y) + 0.5));
            }
            push_clip(input,
                parent_clip,
                tile_ix,
                local_ix,
                clip_depth,
                clip_stack0,
                clip_stack1,
                clip_stack2,
                clip_stack3);
            clip_mask = combine_alpha(parent_clip, alpha);
        } else if (tag == PTCL_END_OPACITY || tag == PTCL_END_BLEND) {
            FineGroup value;
            if (pop_group(input, tile_ix, local_ix, group_depth, group0, group1, value)) {
                if (value.kind == PTCL_BEGIN_OPACITY) {
                    uint alpha = combine_alpha(combine_alpha(value.layer_alpha, value.parent_clip), value.payload);
                    pixel = src_over_premul_unorm(value.parent_pixel, scale_premul_u8_to_unorm(unorm_to_rgba8(pixel), alpha));
                } else if (value.kind == PTCL_BEGIN_BLEND) {
                    uint alpha = combine_alpha(value.layer_alpha, value.parent_clip);
                    uint src = scale_premul_u8(unorm_to_rgba8(pixel), alpha);
                    if ((src >> 24u) == 0u) pixel = value.parent_pixel;
                    else pixel = rgba8_to_unorm(blend_premul_u8(unorm_to_rgba8(value.parent_pixel), src, value.payload));
                }
            }
        } else if (
            tag == PTCL_FILL ||
            tag == PTCL_PATH_GLYPH ||
            tag == PTCL_BEGIN_CLIP ||
            tag == PTCL_BEGIN_OPACITY ||
            tag == PTCL_BEGIN_BLEND
        ) {
            uint alpha = fill_alpha_at(input.segments,
                ptcl.backdrop,
                ptcl.fill_rule,
                ptcl.segment_start,
                ptcl.segment_end,
                local_x,
                local_y);
            if (tag == PTCL_BEGIN_CLIP) {
                push_clip(input,
                    clip_mask,
                    tile_ix,
                    local_ix,
                    clip_depth,
                    clip_stack0,
                    clip_stack1,
                    clip_stack2,
                    clip_stack3);
                clip_mask = combine_alpha(clip_mask, alpha);
            } else if (tag == PTCL_BEGIN_OPACITY || tag == PTCL_BEGIN_BLEND) {
                FineGroup value;
                value.kind = tag;
                value.parent_pixel = pixel;
                value.parent_clip = clip_mask;
                value.layer_alpha = alpha;
                value.payload = ptcl.color;
                if (push_group(input, tile_ix, local_ix, value, group_depth, group0, group1)) {
                    pixel = float4(0.0,0.0,0.0,0.0);
                }
            } else {
                uint masked_alpha = combine_alpha(alpha, clip_mask);
                if (masked_alpha != 0u) {
                    uint draw_ix = ptcl.color;
                    FineDraw draw = load_fine_draw(input.draws,draw_ix);
                    uint color = sample_draw_brush(input, images, draw, float(global_x) + 0.5, float(global_y) + 0.5);
                    if (tag == PTCL_PATH_GLYPH) {
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

#endif
