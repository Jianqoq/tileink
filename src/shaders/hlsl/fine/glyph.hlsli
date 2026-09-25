#ifndef TILEINK_HLSL_FINE_GLYPH_INCLUDED
#define TILEINK_HLSL_FINE_GLYPH_INCLUDED
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
#include "../shared/affine.hlsli"
#include "text/basic.hlsli"
#include "text/auto.hlsli"

float4 composite_glyphs_at(FineInputs input, Texture2D<float4> images[NATIVE_TEXTURE_TABLE_CAPACITY], float4 start_pixel,
    uint glyph_start,
    uint glyph_end,
    FineDraw draw,
    uint global_x,
    uint global_y,
    uint clip_mask) {
    float4 pixel = start_pixel;
    uint glyph_list_ix = glyph_start;
    float2 local = affine_record_point(
        draw.inverse_transform,
        float2(float(global_x) + 0.5, float(global_y) + 0.5));
    int px = int(floor(local.x));
    int py = int(floor(local.y));
    while (true) {
        if (glyph_list_ix >= glyph_end) {
            break;
        }
        uint glyph_i = coarse_load_glyph(input, glyph_list_ix);
        FineGlyph glyph = glyph_at(input, glyph_i);
        uint image_id = glyph.image_id;
        if (image_id != INVALID_INDEX) {
            FineGlyphImage image = glyph_image_at(input, image_id);
            uint width = image.width;
            uint height = image.height;
            int x0 = glyph.x + image.left;
            int y0 = glyph.y - image.top;
            int local_x = px - x0;
            int local_y = py - y0;
            if (local_x >= 0 && local_y >= 0 && local_x < int(width) && local_y < int(height)) {
                uint data_ix = image.data_offset + uint(local_y) * width + uint(local_x);
                uint content = image.content;
                uint data = glyph_image_data_at(input, data_ix);
                if (content == GLYPH_MASK) {
                    uint alpha = combine_alpha(data, clip_mask);
                    if (alpha != 0u) {
                        uint color = sample_draw_brush(input, images, draw, float(global_x) + 0.5, float(global_y) + 0.5);
                        pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(color, alpha));
                    }
                } else if (content == GLYPH_LINEAR_MASK) {
                    uint alpha = combine_alpha(data, clip_mask);
                    if (alpha != 0u) {
                        uint color = sample_draw_brush(input, images, draw, float(global_x) + 0.5, float(global_y) + 0.5);
                        pixel = rgba8_to_unorm(src_over_mask_linear_auto_u8(unorm_to_rgba8(pixel), color, alpha));
                    }
                } else if (content == GLYPH_COLOR) {
                    pixel = src_over_premul_unorm(pixel, scale_premul_u8_to_unorm(data, clip_mask));
                } else if (content == GLYPH_LINEAR_COLOR) {
                    pixel = rgba8_to_unorm(src_over_mask_linear_auto_u8(unorm_to_rgba8(pixel), data, clip_mask));
                } else if (content == GLYPH_SUBPIXEL_MASK) {
                    uint color = sample_draw_brush(input, images, draw, float(global_x) + 0.5, float(global_y) + 0.5);
                    pixel = rgba8_to_unorm(src_over_subpixel_mask_u8(unorm_to_rgba8(pixel), color, data, clip_mask));
                } else if (content == GLYPH_LINEAR_SUBPIXEL_MASK) {
                    uint color = sample_draw_brush(input, images, draw, float(global_x) + 0.5, float(global_y) + 0.5);
                    pixel = rgba8_to_unorm(src_over_subpixel_mask_linear_auto_u8(unorm_to_rgba8(pixel), color, data, clip_mask));
                }
            }
        }
        glyph_list_ix += 1u;
    }
    return pixel;
}

#endif
