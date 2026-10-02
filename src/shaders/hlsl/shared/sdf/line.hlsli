#ifndef TILEINK_HLSL_SDF_LINE_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_LINE_HLSLI_INCLUDED
#include "base.hlsli"
#include "basic.hlsli"
#include "types.hlsli"
#include "../pixel.hlsli"

SdfSample line_local_to_canvas_sample(SdfSample sdf_sample, float ux, float uy) {
    return make_sdf_sample(
        sdf_sample.distance,
        float2(
            sdf_sample.normal.x * ux - sdf_sample.normal.y * uy,
            sdf_sample.normal.x * uy + sdf_sample.normal.y * ux));
}

SdfSample line_segment_sdf_sample(float axis, float normal, float start, float end, float half_extent, float cap) {
    float nearest = clamp(axis, start, end);
    float2 delta = float2(axis - nearest, normal);
    SdfSample sdf_sample = make_sdf_sample(length(delta) - half_extent, normalized_or(delta, float2(0.0, 1.0)));
    if (cap < 0.5) {
        sdf_sample = local_line_rect_sample(axis, normal, start, end, half_extent);
    } else if (cap < 1.5) {
        sdf_sample = local_line_rect_sample(axis, normal, start - half_extent, end + half_extent, half_extent);
    }
    return sdf_sample;
}

SdfSample dash_line_segment_sample(float axis, float normal, float len, float half_extent, float cap, float dash_length, float cycle, float offset, float dash_ix) {
    float dash_start = dash_ix * cycle - offset;
    float dash_end = dash_start + dash_length;
    SdfSample sdf_sample = make_sdf_sample(1000000.0, float2(1.0, 0.0));
    if (dash_end > 0.0 && dash_start < len) {
        float start = max(dash_start, 0.0);
        float end = min(dash_end, len);
        if (end > start) {
            sdf_sample = line_segment_sdf_sample(axis, normal, start, end, half_extent, cap);
        }
    }
    return sdf_sample;
}

SdfSample line_sdf_sample(float x, float y, float sx, float sy, float ex, float ey, float width, float cap) {
    float half_extent = max(width, 0.0) * 0.5;
    float dx = ex - sx;
    float dy = ey - sy;
    float len = sqrt(dx * dx + dy * dy);
    SdfSample sdf_sample = make_sdf_sample(1000000.0, float2(1.0, 0.0));
    if (len <= 0.000001) {
        if (cap >= 0.5) {
            if (cap > 1.5) {
                float2 delta = float2(x - sx, y - sy);
                sdf_sample = make_sdf_sample(length(delta) - half_extent, normalized_or(delta, float2(1.0, 0.0)));
            } else {
                sdf_sample = local_line_rect_sample(x - sx, y - sy, -half_extent, half_extent, half_extent);
            }
        }
    } else {
        float ux = dx / len;
        float uy = dy / len;
        float px = x - sx;
        float py = y - sy;
        float axis = px * ux + py * uy;
        float normal = -px * uy + py * ux;
        if (cap < 0.5) {
            sdf_sample = local_line_rect_sample(axis, normal, 0.0, len, half_extent);
        } else if (cap < 1.5) {
            sdf_sample = local_line_rect_sample(axis, normal, -half_extent, len + half_extent, half_extent);
        } else {
            float nearest = clamp(axis, 0.0, len);
            float2 delta = float2(axis - nearest, normal);
            sdf_sample = make_sdf_sample(length(delta) - half_extent, normalized_or(delta, float2(0.0, 1.0)));
        }
        sdf_sample = line_local_to_canvas_sample(sdf_sample, ux, uy);
    }
    return sdf_sample;
}

SdfSample dash_line_sdf_sample(float x, float y, float sx, float sy, float ex, float ey, float width, float cap, float dash_length_raw, float gap_length_raw, float dash_offset) {
    float dash_length = max(dash_length_raw, 0.0);
    float gap_length = max(gap_length_raw, 0.0);
    SdfSample sdf_sample = line_sdf_sample(x, y, sx, sy, ex, ey, width, cap);
    if (dash_length > 0.000001 && gap_length > 0.000001) {
        float half_extent = max(width, 0.0) * 0.5;
        float dx = ex - sx;
        float dy = ey - sy;
        float len = sqrt(dx * dx + dy * dy);
        if (len > 0.000001) {
            float ux = dx / len;
            float uy = dy / len;
            float px = x - sx;
            float py = y - sy;
            float axis = px * ux + py * uy;
            float normal = -px * uy + py * ux;
            float cycle = dash_length + gap_length;
            float offset = rem_euclid_f32(dash_offset, cycle);
            float base = floor((axis + offset) / cycle);
            SdfSample local_sample = nearer_sdf_sample(
                nearer_sdf_sample(
                    dash_line_segment_sample(axis, normal, len, half_extent, cap, dash_length, cycle, offset, base - 1.0),
                    dash_line_segment_sample(axis, normal, len, half_extent, cap, dash_length, cycle, offset, base)),
                dash_line_segment_sample(axis, normal, len, half_extent, cap, dash_length, cycle, offset, base + 1.0));
            sdf_sample = line_local_to_canvas_sample(local_sample, ux, uy);
        }
    }
    return sdf_sample;
}

#endif // TILEINK_HLSL_SDF_LINE_HLSLI_INCLUDED
