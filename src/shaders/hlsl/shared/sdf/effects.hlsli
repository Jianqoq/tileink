#ifndef TILEINK_HLSL_SDF_EFFECTS_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_EFFECTS_HLSLI_INCLUDED
#include "base.hlsli"
#include "basic.hlsli"
#include "types.hlsli"
#include "../pixel.hlsli"

SdfSample checkerboard_sdf_sample(float x, float y, float x0, float y0, float x1, float y1, float cell_size_raw) {
    float left = min(x0, x1);
    float top = min(y0, y1);
    float right = max(x0, x1);
    float bottom = max(y0, y1);
    SdfSample outer = rect_sdf_sample(x, y, left, top, right, bottom, 0.0, 0.0, 0.0, 0.0);
    float cell_size = max(cell_size_raw, 0.000001);
    float cell_x = floor((x - left) / cell_size);
    float cell_y = floor((y - top) / cell_size);
    float local_x = rem_euclid_f32(x - left, cell_size);
    float local_y = rem_euclid_f32(y - top, cell_size);
    float edge_distance = local_x;
    float2 normal = float2(1.0, 0.0);
    if (cell_size - local_x < edge_distance) {
        edge_distance = cell_size - local_x;
        normal = float2(-1.0, 0.0);
    }
    if (local_y < edge_distance) {
        edge_distance = local_y;
        normal = float2(0.0, 1.0);
    }
    if (cell_size - local_y < edge_distance) {
        edge_distance = cell_size - local_y;
        normal = float2(0.0, -1.0);
    }
    bool filled = (int(cell_x) + int(cell_y)) % 2 == 0;
    SdfSample cell = make_sdf_sample(((filled) ? (-edge_distance) : (edge_distance)), normal);
    return farther_sdf_sample(outer, cell);
}

float candlestick_sdf_coverage(float x, float y, float center_x, float high_y, float low_y, float body_top_y, float body_bottom_y, float body_width, float wick_width, AffineRecord inverse_transform) {
    float wick_half_width = max(wick_width, 1.0) * 0.5;
    float wick = sdf_coverage_from_sample(rect_sdf_sample(
        x,
        y,
        center_x - wick_half_width,
        min(high_y, low_y),
        center_x + wick_half_width,
        max(high_y, low_y),
        0.0,
        0.0,
        0.0,
        0.0), inverse_transform);
    float half_width = max(body_width, 1.0) * 0.5;
    float body_y0 = min(body_top_y, body_bottom_y);
    float body_y1 = max(body_top_y, body_bottom_y);
    if (body_y0 == body_y1) {
        body_y0 = body_y0 - 0.5;
        body_y1 = body_y1 + 0.5;
    }
    float body = sdf_coverage_from_sample(rect_sdf_sample(
        x,
        y,
        center_x - half_width,
        body_y0,
        center_x + half_width,
        body_y1,
        0.0,
        0.0,
        0.0,
        0.0), inverse_transform);
    return max(wick, body);
}

#endif // TILEINK_HLSL_SDF_EFFECTS_HLSLI_INCLUDED
