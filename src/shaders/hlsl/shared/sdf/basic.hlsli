#ifndef TILEINK_HLSL_SDF_BASIC_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_BASIC_HLSLI_INCLUDED
#include "base.hlsli"
#include "types.hlsli"

SdfSample rounded_box_sdf_sample(float px, float py, float hx, float hy, float radius) {
    float2 q = float2(abs(px) - hx + radius, abs(py) - hy + radius);
    float2 outside = max(q, float2(0.0,0.0));
    float distance = min(max(q.x, q.y), 0.0) + length(outside) - radius;
    float2 signs = float2(((px >= 0.0) ? (1.0) : (-1.0)), ((py >= 0.0) ? (1.0) : (-1.0)));
    if (dot(outside, outside) > 0.000000000001) {
        return make_sdf_sample(distance, normalize(outside) * signs);
    }
    if (q.x > q.y) {
        return make_sdf_sample(distance, float2(signs.x, 0.0));
    }
    return make_sdf_sample(distance, float2(0.0, signs.y));
}

SdfSample local_line_rect_sample(float axis, float normal, float x0, float x1, float half_height) {
    float center = (x0 + x1) * 0.5;
    float half_width = (x1 - x0) * 0.5;
    return rounded_box_sdf_sample(axis - center, normal, half_width, half_height, 0.0);
}

SdfSample circle_sdf_sample(float x, float y, float cx, float cy, float radius) {
    float dx = x - cx;
    float dy = y - cy;
    float2 delta = float2(dx, dy);
    return make_sdf_sample(length(delta) - radius, normalized_or(delta, float2(1.0, 0.0)));
}

SdfSample rect_sdf_sample(float x, float y, float x0_raw, float y0_raw, float x1_raw, float y1_raw, float top_left, float top_right, float bottom_left, float bottom_right) {
    float x0 = min(x0_raw, x1_raw);
    float y0 = min(y0_raw, y1_raw);
    float x1 = max(x0_raw, x1_raw);
    float y1 = max(y0_raw, y1_raw);
    float cx = (x0 + x1) * 0.5;
    float cy = (y0 + y1) * 0.5;
    float hx = (x1 - x0) * 0.5;
    float hy = (y1 - y0) * 0.5;
    float px = x - cx;
    float py = y - cy;
    float radius = top_left;
    if (px >= 0.0) {
        if (py <= 0.0) {
            radius = top_right;
        } else {
            radius = bottom_right;
        }
    } else if (py > 0.0) {
        radius = bottom_left;
    }
    float r = max(min(min(radius, hx), hy), 0.0);
    return rounded_box_sdf_sample(px, py, hx, hy, r);
}

float rect_sdf_distance(float x, float y, float x0_raw, float y0_raw, float x1_raw, float y1_raw, float top_left, float top_right, float bottom_left, float bottom_right) {
    return rect_sdf_sample(
        x,
        y,
        x0_raw,
        y0_raw,
        x1_raw,
        y1_raw,
        top_left,
        top_right,
        bottom_left,
        bottom_right).distance;
}

#endif // TILEINK_HLSL_SDF_BASIC_HLSLI_INCLUDED
