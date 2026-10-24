#ifndef TILEINK_HLSL_SDF_COVERAGE_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_COVERAGE_HLSLI_INCLUDED
#include "arc.hlsli"
#include "base.hlsli"
#include "basic.hlsli"
#include "constants.hlsli"
#include "data.hlsli"
#include "effects.hlsli"
#include "line.hlsli"
#include "polygon.hlsli"
#include "types.hlsli"

float sdf_coverage_from_blob(ByteAddressBuffer paint, uint base, float x, float y, AffineRecord inverse_transform) {
    uint kind = sdf_word(paint, base, 0u);
    float x0 = sdf_float(paint, base, 1u);
    float y0 = sdf_float(paint, base, 2u);
    float x1 = sdf_float(paint, base, 3u);
    float y1 = sdf_float(paint, base, 4u);
    float r0 = sdf_float(paint, base, 5u);
    float r1 = sdf_float(paint, base, 6u);
    float r2 = sdf_float(paint, base, 7u);
    float r3 = sdf_float(paint, base, 8u);
    float stroke_top = sdf_float(paint, base, 9u);
    float stroke_right = sdf_float(paint, base, 10u);
    float stroke_bottom = sdf_float(paint, base, 11u);
    float stroke_left = sdf_float(paint, base, 12u);
    float shadow_offset_x = sdf_float(paint, base, 13u);
    float shadow_offset_y = sdf_float(paint, base, 14u);
    float shadow_expand = sdf_float(paint, base, 15u);
    float shadow_intensity = sdf_float(paint, base, 16u);

    if (kind == SDF_KIND_RECT) {
        return sdf_coverage_from_sample(rect_sdf_sample(x, y, x0, y0, x1, y1, r0, r1, r2, r3), inverse_transform);
    }
    if (kind == SDF_KIND_RECT_STROKE) {
        float half_top = max(stroke_top, 0.0);
        float half_right = max(stroke_right, 0.0);
        float half_bottom = max(stroke_bottom, 0.0);
        float half_left = max(stroke_left, 0.0);
        float rx0 = min(x0, x1);
        float ry0 = min(y0, y1);
        float rx1 = max(x0, x1);
        float ry1 = max(y0, y1);
        float outer = sdf_coverage_from_sample(rect_sdf_sample(
            x,
            y,
            rx0 - half_left,
            ry0 - half_top,
            rx1 + half_right,
            ry1 + half_bottom,
            r0 + max(half_top, half_left),
            r1 + max(half_top, half_right),
            r2 + max(half_bottom, half_left),
            r3 + max(half_bottom, half_right)), inverse_transform);
        float inner_x0 = rx0 + half_left;
        float inner_y0 = ry0 + half_top;
        float inner_x1 = rx1 - half_right;
        float inner_y1 = ry1 - half_bottom;
        float inner = 0.0;
        if (inner_x0 < inner_x1 && inner_y0 < inner_y1) {
            inner = sdf_coverage_from_sample(rect_sdf_sample(
                x,
                y,
                inner_x0,
                inner_y0,
                inner_x1,
                inner_y1,
                max(r0 - max(half_top, half_left), 0.0),
                max(r1 - max(half_top, half_right), 0.0),
                max(r2 - max(half_bottom, half_left), 0.0),
                max(r3 - max(half_bottom, half_right), 0.0)), inverse_transform);
        }
        return clamp(outer - inner, 0.0, 1.0);
    }
    if (kind == SDF_KIND_RECT_SHADOW) {
        return sdf_shadow_coverage_from_sample(
            rect_sdf_sample(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
                r3),
            inverse_transform,
            shadow_expand,
            shadow_intensity);
    }
    if (kind == SDF_KIND_CIRCLE) {
        return sdf_coverage_from_sample(circle_sdf_sample(x, y, x0, y0, x1), inverse_transform);
    }
    if (kind == SDF_KIND_CIRCLE_STROKE) {
        float half_extent = max(stroke_top, 0.0);
        float radius = max(x1, 0.0);
        float outer = sdf_coverage_from_sample(circle_sdf_sample(x, y, x0, y0, radius + half_extent), inverse_transform);
        float inner = 0.0;
        if (radius > half_extent) {
            inner = sdf_coverage_from_sample(circle_sdf_sample(x, y, x0, y0, radius - half_extent), inverse_transform);
        }
        return clamp(outer - inner, 0.0, 1.0);
    }
    if (kind == SDF_KIND_CIRCLE_SHADOW) {
        return sdf_shadow_coverage_from_sample(
            circle_sdf_sample(x - shadow_offset_x, y - shadow_offset_y, x0, y0, x1),
            inverse_transform,
            shadow_expand,
            shadow_intensity);
    }
    if (kind == SDF_KIND_ARC) {
        return sdf_coverage_from_sample(arc_sdf_sample(x, y, x0, y0, x1, y1, r0, r1, r2), inverse_transform);
    }
    if (kind == SDF_KIND_ARC_SHADOW) {
        return sdf_shadow_coverage_from_sample(
            arc_sdf_sample(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2),
            inverse_transform,
            shadow_expand,
            shadow_intensity);
    }
    if (kind == SDF_KIND_CANDLESTICK) {
        return candlestick_sdf_coverage(x, y, x0, y0, x1, y1, r0, r1, r2, inverse_transform);
    }
    if (kind == SDF_KIND_LINE) {
        return sdf_coverage_from_sample(line_sdf_sample(x, y, x0, y0, x1, y1, r0, r1), inverse_transform);
    }
    if (kind == SDF_KIND_DASH_LINE) {
        return sdf_coverage_from_sample(
            dash_line_sdf_sample(x, y, x0, y0, x1, y1, r0, r1, r2, r3, stroke_top),
            inverse_transform);
    }
    if (kind == SDF_KIND_TRIANGLE) {
        return sdf_coverage_from_sample(
            triangle_sdf_sample(x, y, x0, y0, x1, y1, r0, r1, r2),
            inverse_transform);
    }
    if (kind == SDF_KIND_CHECKERBOARD) {
        return sdf_coverage_from_sample(
            checkerboard_sdf_sample(x, y, x0, y0, x1, y1, r0),
            inverse_transform);
    }
    if (kind == SDF_KIND_STAR) {
        return sdf_coverage_from_sample(
            star_sdf_sample(x, y, x0, y0, x1, y1, r0, r1),
            inverse_transform);
    }
    if (kind == SDF_KIND_STAR_STROKE) {
        return sdf_coverage_from_sample(
            star_stroke_sdf_sample(x, y, x0, y0, x1, y1, r0, r1, stroke_top),
            inverse_transform);
    }
    if (kind == SDF_KIND_CALLOUT) {
        return sdf_coverage_from_sample(
            callout_sdf_sample(
                x,
                y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
                r3,
                stroke_top,
                stroke_right,
                stroke_left),
            inverse_transform);
    }
    if (kind == SDF_KIND_CALLOUT_STROKE) {
        return sdf_coverage_from_sample(
            callout_stroke_sdf_sample(
                x,
                y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
                r3,
                stroke_top,
                stroke_right,
                stroke_bottom,
                stroke_left),
            inverse_transform);
    }
    if (kind == SDF_KIND_CALLOUT_SHADOW) {
        return sdf_shadow_coverage_from_sample(
            callout_sdf_sample(
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
                stroke_top,
                stroke_right,
                stroke_left),
            inverse_transform,
            shadow_expand,
            shadow_intensity);
    }
    if (kind == SDF_KIND_LINE_SHADOW) {
        return sdf_shadow_coverage_from_sample(
            line_sdf_sample(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1),
            inverse_transform,
            shadow_expand,
            shadow_intensity);
    }
    return 0.0;
}

#endif // TILEINK_HLSL_SDF_COVERAGE_HLSLI_INCLUDED
