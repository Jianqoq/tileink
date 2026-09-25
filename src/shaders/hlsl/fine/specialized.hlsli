#ifndef TILEINK_HLSL_FINE_SPECIALIZED_INCLUDED
#define TILEINK_HLSL_FINE_SPECIALIZED_INCLUDED
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
#include "../shared/brush/data.hlsli"
#include "../shared/brush/constants.hlsli"

// A failed specialization leaves the root entry to run the interpreter once.
// Do not inline the complete glyph/stack interpreter into each fast path.
bool color_only_no_stack_tile_pixel(FineInputs input, uint tile_ix, uint local_ix, out float4 pixel) {
    uint tile_x = tile_ix % input.config.tiles_width;
    uint tile_y = tile_ix / input.config.tiles_width;
    uint global_x = tile_x * TILE_SIZE + local_ix % TILE_SIZE;
    uint global_y = tile_y * TILE_SIZE + local_ix / TILE_SIZE;
    pixel = fine_initial_pixel(input, global_x, global_y);

    FineTile tile = coarse_load_tile(input, tile_ix);
    uint ptcl_ix = tile.ptcl_start;
    while (true) {
        if (ptcl_ix >= tile.ptcl_end) {
            break;
        }
        FineParticle ptcl = coarse_load_ptcl(input, ptcl_ix);
        uint tag = ptcl.tag;
        if (tag == PTCL_END) {
            break;
        }
        if (tag != PTCL_COLOR) {
            return false;
        }
        if (premul_u8_is_opaque(ptcl.color)) {
            pixel = rgba8_to_unorm(ptcl.color);
        } else {
            pixel = src_over_premul_unorm(pixel, rgba8_to_unorm(ptcl.color));
        }
        ptcl_ix += 1u;
    }
    return true;
}

bool analytic_solid_no_stack_tile_pixel(FineInputs input, Texture2D<float4> images[NATIVE_TEXTURE_TABLE_CAPACITY], uint tile_ix, uint local_ix, out float4 pixel) {
    uint tile_x = tile_ix % input.config.tiles_width;
    uint tile_y = tile_ix / input.config.tiles_width;
    uint global_x = tile_x * TILE_SIZE + local_ix % TILE_SIZE;
    uint global_y = tile_y * TILE_SIZE + local_ix / TILE_SIZE;
    float sample_x = float(global_x) + 0.5;
    float sample_y = float(global_y) + 0.5;
    pixel = fine_initial_pixel(input, global_x, global_y);

    FineTile tile = coarse_load_tile(input, tile_ix);
    if (tile.ptcl_start + 1u < tile.ptcl_end) {
        FineParticle first = coarse_load_ptcl(input, tile.ptcl_start);
        FineParticle second = coarse_load_ptcl(input, tile.ptcl_start + 1u);
        if (first.tag == PTCL_IMAGE && second.tag == PTCL_END) {
            uint color = sample_draw_brush(input, images, load_fine_draw(input.draws,first.color), sample_x, sample_y);
            if (premul_u8_is_opaque(color)) {
                pixel = rgba8_to_unorm(color); return true;
            }
            pixel = src_over_premul_unorm(pixel, rgba8_to_unorm(color)); return true;
        }
    }
    uint ptcl_ix = tile.ptcl_start;
    while (true) {
        if (ptcl_ix >= tile.ptcl_end) {
            break;
        }
        FineParticle ptcl = coarse_load_ptcl(input, ptcl_ix);
        uint tag = ptcl.tag;
        if (tag == PTCL_END) {
            break;
        }
        if (tag == PTCL_COLOR) {
            if (premul_u8_is_opaque(ptcl.color)) {
                pixel = rgba8_to_unorm(ptcl.color);
            } else {
                pixel = src_over_premul_unorm(pixel, rgba8_to_unorm(ptcl.color));
            }
        } else if (tag == PTCL_IMAGE) {
            FineDraw draw = load_fine_draw(input.draws,ptcl.color);
            uint color = sample_draw_brush(input, images, draw, sample_x, sample_y);
            if (premul_u8_is_opaque(color)) {
                pixel = rgba8_to_unorm(color);
            } else {
                pixel = src_over_premul_unorm(pixel, rgba8_to_unorm(color));
            }
        } else if (tag == PTCL_SDF) {
            FineDraw draw = load_fine_draw(input.draws,ptcl.color);
            float coverage = 0.0;
            if (pixel_in_draw_bounds(draw, global_x, global_y)) {
                coverage = sdf_coverage_from_draw(input, draw, sample_x, sample_y);
            }
            uint alpha = coverage_to_u8(coverage);
            if (alpha != 0u) {
                uint color = brush_word(input.paint,input.config.paint_brush_base,draw.brush_offset + BRUSH_COLOR_WORD);
                if (alpha == 255u && premul_u8_is_opaque(color)) {
                    pixel = rgba8_to_unorm(color);
                } else {
                    pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(color, alpha));
                }
            }
        } else {
            return false;
        }
        ptcl_ix += 1u;
    }
    return true;
}

#endif
