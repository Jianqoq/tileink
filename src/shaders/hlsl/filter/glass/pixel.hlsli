#ifndef TILEINK_FILTER_GLASS_PIXEL_HLSLI
#define TILEINK_FILTER_GLASS_PIXEL_HLSLI
#include "constants.hlsli"
#include "../config.hlsli"
#include "../../shared/pixel.hlsli"
#include "color.hlsli"
#include "geometry.hlsli"
#include "sample.hlsli"
uint liquid_glass_pixel(ConstantBuffer<FilterConfig> config, Texture2D<float4> source, Texture2D<float4> auxiliary, uint base, float world_x, float world_y, float pixel_x, float pixel_y, float distance, float distance_norm, float surface_height) {
    float2 normal = liquid_glass_normal(config, world_x, world_y);
    float nx = normal.x;
    float ny = normal.y;
    float inside_distance = -distance;
    float edge = liquid_glass_edge(
        inside_distance,
        config.liquid_refraction_thickness,
        config.liquid_refraction_factor);
    float blur_mix = inside_distance / max(config.liquid_refraction_thickness, LIQUID_GLASS_EPSILON);
    if (config.mask_enabled == 1u) {
        blur_mix = 1.0;
    }
    blur_mix = clamp(blur_mix, 0.0, 1.0);

    float normal_len = LIQUID_GLASS_NORMAL_LENGTH_SCALE / surface_height;
    float4 initial_blur = liquid_glass_sample_straight_rgba(config, source, auxiliary, 1u, pixel_x, pixel_y);
    float r = initial_blur.r;
    float g = initial_blur.g;
    float b = initial_blur.b;
    float a = initial_blur.a;
    float tint_mix = config.liquid_tint_a * LIQUID_GLASS_TINT_MIX;
    float tint_base_mix = config.liquid_tint_a * LIQUID_GLASS_TINT_BASE_MIX;

    if (edge <= 0.0) {
        if (tint_mix > 0.0) {
            r = glass_lerp(r, config.liquid_tint_r, tint_mix);
            g = glass_lerp(g, config.liquid_tint_g, tint_mix);
            b = glass_lerp(b, config.liquid_tint_b, tint_mix);
            a = glass_lerp(a, 1.0, tint_mix);
        }
    } else {
        // A zero normal component has exactly zero displacement even when the
        // other component overflows. Guard before multiplication so compiler
        // reassociation cannot turn zero * infinity into a NaN sample coordinate.
        float offset_x = 0.0;
        float offset_y = 0.0;
        if (nx != 0.0) { offset_x = -nx * edge * LIQUID_GLASS_REFRACTION_PIXEL_SCALE; }
        if (ny != 0.0) { offset_y = -ny * edge * LIQUID_GLASS_REFRACTION_PIXEL_SCALE; }
        if (abs(config.liquid_refraction_dispersion) <= LIQUID_GLASS_EPSILON) {
            float sx = pixel_x + offset_x;
            float sy = pixel_y + offset_y;
            float4 src_rgba = liquid_glass_sample_straight_rgba(config, source, auxiliary, 0u, sx, sy);
            float4 blur_rgba = liquid_glass_sample_straight_rgba(config, source, auxiliary, 1u, sx, sy);
            r = glass_lerp(src_rgba.r, blur_rgba.r, blur_mix);
            g = glass_lerp(src_rgba.g, blur_rgba.g, blur_mix);
            b = glass_lerp(src_rgba.b, blur_rgba.b, blur_mix);
            a = max(src_rgba.a, blur_rgba.a);
        } else {
            float sx = pixel_x + offset_x;
            float sy = pixel_y + offset_y;
            float4 src_rgba = liquid_glass_sample_straight_rgba(config, source, auxiliary, 0u, sx, sy);
            float4 blur_rgba = liquid_glass_sample_straight_rgba(config, source, auxiliary, 1u, sx, sy);
            r = liquid_glass_dispersion_channel(config, source, auxiliary, pixel_x, pixel_y, offset_x, offset_y, LIQUID_GLASS_CHROMATIC_R, 0u, blur_mix);
            g = glass_lerp(src_rgba.g, blur_rgba.g, blur_mix);
            b = liquid_glass_dispersion_channel(config, source, auxiliary, pixel_x, pixel_y, offset_x, offset_y, LIQUID_GLASS_CHROMATIC_B, 2u, blur_mix);
            a = max(src_rgba.a, blur_rgba.a);
        }
        float blurred_r = r;
        float blurred_g = g;
        float blurred_b = b;
        if (tint_mix > 0.0) {
            r = glass_lerp(r, config.liquid_tint_r, tint_mix);
            g = glass_lerp(g, config.liquid_tint_g, tint_mix);
            b = glass_lerp(b, config.liquid_tint_b, tint_mix);
            a = glass_lerp(a, 1.0, tint_mix);
        }

        if (config.liquid_fresnel_factor > 0.0) {
            float fresnel = liquid_glass_highlight_geometry(distance, config.liquid_fresnel_range, config.liquid_fresnel_hardness);
            float fresnel_base_r = glass_lerp(1.0, config.liquid_tint_r, tint_base_mix);
            float fresnel_base_g = glass_lerp(1.0, config.liquid_tint_g, tint_base_mix);
            float fresnel_base_b = glass_lerp(1.0, config.liquid_tint_b, tint_base_mix);
            float fresnel_l = liquid_glass_srgb_to_lch_l(fresnel_base_r, fresnel_base_g, fresnel_base_b);
            float fresnel_c = liquid_glass_srgb_to_lch_c(fresnel_base_r, fresnel_base_g, fresnel_base_b);
            float fresnel_h = liquid_glass_srgb_to_lch_h(fresnel_base_r, fresnel_base_g, fresnel_base_b);
            fresnel_l = clamp(fresnel_l + LIQUID_GLASS_FRESNEL_LIGHTNESS_GAIN * fresnel * config.liquid_fresnel_factor, 0.0, 100.0);
            float fresnel_mix = fresnel * config.liquid_fresnel_factor * LIQUID_GLASS_FRESNEL_MIX_SCALE * normal_len;
            r = glass_lerp(r, liquid_glass_lch_to_srgb_r(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
            g = glass_lerp(g, liquid_glass_lch_to_srgb_g(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
            b = glass_lerp(b, liquid_glass_lch_to_srgb_b(fresnel_l, fresnel_c, fresnel_h), fresnel_mix);
            a = glass_lerp(a, 1.0, fresnel_mix);
        }

        if (config.liquid_glare_factor > 0.0) {
            float glare_geo = liquid_glass_highlight_geometry(distance, config.liquid_glare_range, config.liquid_glare_hardness);
            float glare_angle_factor = liquid_glass_glare_angle(config, nx, ny);
            float glare_base_r = glass_lerp(blurred_r, config.liquid_tint_r, tint_base_mix);
            float glare_base_g = glass_lerp(blurred_g, config.liquid_tint_g, tint_base_mix);
            float glare_base_b = glass_lerp(blurred_b, config.liquid_tint_b, tint_base_mix);
            float glare_l = liquid_glass_srgb_to_lch_l(glare_base_r, glare_base_g, glare_base_b);
            float glare_c = liquid_glass_srgb_to_lch_c(glare_base_r, glare_base_g, glare_base_b);
            float glare_h = liquid_glass_srgb_to_lch_h(glare_base_r, glare_base_g, glare_base_b);
            glare_l = clamp(glare_l + LIQUID_GLASS_GLARE_LIGHTNESS_GAIN * glare_angle_factor * glare_geo, 0.0, 120.0);
            glare_c += LIQUID_GLASS_GLARE_CHROMA_GAIN * glare_angle_factor * glare_geo;
            float glare_mix = glare_angle_factor * glare_geo * normal_len;
            r = glass_lerp(r, liquid_glass_lch_to_srgb_r(glare_l, glare_c, glare_h), glare_mix);
            g = glass_lerp(g, liquid_glass_lch_to_srgb_g(glare_l, glare_c, glare_h), glare_mix);
            b = glass_lerp(b, liquid_glass_lch_to_srgb_b(glare_l, glare_c, glare_h), glare_mix);
            a = glass_lerp(a, 1.0, glare_mix);
        }
    }

    float edge_mix = liquid_glass_smoothstep(LIQUID_GLASS_EDGE_BLEND_START, LIQUID_GLASS_EDGE_BLEND_END, distance_norm);
    r = glass_lerp(r, liquid_glass_pixel_straight_channel(base, 0u), edge_mix);
    g = glass_lerp(g, liquid_glass_pixel_straight_channel(base, 1u), edge_mix);
    b = glass_lerp(b, liquid_glass_pixel_straight_channel(base, 2u), edge_mix);
    a = glass_lerp(a, liquid_glass_pixel_straight_channel(base, 3u), edge_mix);
    return liquid_glass_pack_straight_rgba8(r, g, b, a);
}
#endif
