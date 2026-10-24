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

// Match Metal: preserve fractional affine motion while keeping integer texels exact.
uint glyph_texel(FineInputs input, FineGlyphImage image, int2 p) {
    if (any(p < 0) || p.x >= int(image.width) || p.y >= int(image.height)) return 0u;
    return glyph_image_data_at(input, image.data_offset + uint(p.y) * image.width + uint(p.x));
}
uint sample_glyph(FineInputs input, FineGlyphImage image, float2 p) {
    int2 base = int2(floor(p));
    float2 f = p - float2(base);
    uint a = glyph_texel(input, image, base);
    if (all(f == 0.0)) return a;
    float4 top = lerp(rgba8_to_unorm(a), rgba8_to_unorm(glyph_texel(input, image, base + int2(1,0))), f.x);
    float4 bottom = lerp(rgba8_to_unorm(glyph_texel(input, image, base + int2(0,1))), rgba8_to_unorm(glyph_texel(input, image, base + int2(1,1))), f.x);
    return unorm_to_rgba8(lerp(top, bottom, f.y));
}

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
    local -= 0.5;
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
            float local_x = local.x - float(x0);
            float local_y = local.y - float(y0);
            if (local_x > -1.0 && local_y > -1.0 && local_x < int(width) && local_y < int(height)) {
                uint content = image.content;
                uint data = sample_glyph(input, image, float2(local_x, local_y));
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
