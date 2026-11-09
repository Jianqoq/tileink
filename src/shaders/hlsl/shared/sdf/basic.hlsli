#ifndef TILEINK_HLSL_SDF_BASIC_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_BASIC_HLSLI_INCLUDED
#include "base.hlsli"
#include "types.hlsli"

SdfSample rounded_box_sdf_sample(float px, float py, float hx, float hy, float radius) {
    float2 q = float2(abs(px) - hx + radius, abs(py) - hy + radius);
    float2 outside = max(q, float2(0.0,0.0));
    float outside_length = sdf_length(outside);
    float distance = min(max(q.x, q.y), 0.0) + outside_length - radius;
    float2 signs = float2(((px >= 0.0) ? (1.0) : (-1.0)), ((py >= 0.0) ? (1.0) : (-1.0)));
    if (outside_length > 0.0) {
        return make_sdf_sample(distance, outside / outside_length * signs);
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
    return make_sdf_sample(sdf_length(delta) - radius, normalized_or(delta, float2(1.0, 0.0)));
}


// CSS Backgrounds 3: constrain adjacent sums with one common scale. Work in
// radius-relative units so a finite oversized input cannot overflow a sum.
float4 css_rect_radii(float4 radii, float2 size) {
    radii = max(radii, float4(0.0, 0.0, 0.0, 0.0));
    float largest = max(max(radii.x, radii.y), max(radii.z, radii.w));
    if (largest == 0.0) return radii;
    float4 unit;
    if (largest > 18446744073709551616.0) {
        // Keep the divisor's reciprocal normal on GPUs that flush float subnormals.
        // 2^-64 is an exact range change, not a geometry tolerance.
        precise float4 scaled = radii * 5.42101086242752217e-20;
        precise float divisor = largest * 5.42101086242752217e-20;
        unit = scaled / divisor;
    } else {
        unit = radii / largest;
    }
    float scale = largest;
    if (unit.x + unit.y > 0.0) scale = min(scale, size.x / (unit.x + unit.y));
    if (unit.z + unit.w > 0.0) scale = min(scale, size.x / (unit.z + unit.w));
    if (unit.x + unit.z > 0.0) scale = min(scale, size.y / (unit.x + unit.z));
    if (unit.y + unit.w > 0.0) scale = min(scale, size.y / (unit.y + unit.w));
    return unit * scale;
}


SdfSample extended_rect_sdf_sample(float2 p, float4 bounds, float4 radii) {
    float2 size = bounds.zw - bounds.xy;
    float2 center = (bounds.xy + bounds.zw) * 0.5;
    float2 q = p - center;
    float4 r = css_rect_radii(radii, size);
    float largest = max(max(r.x, r.y), max(r.z, r.w));
    if (largest <= min(size.x, size.y) * 0.5) {
        float radius = q.x >= 0.0 ? (q.y <= 0.0 ? r.y : r.w) : (q.y > 0.0 ? r.z : r.x);
        return rounded_box_sdf_sample(q.x, q.y, size.x * 0.5, size.y * 0.5, radius);
    }
    // A large corner crosses the midpoint, so quadrant selection is insufficient.
    // Intersect the box with each applicable circular cut and retain its true normal.
    SdfSample result = rounded_box_sdf_sample(q.x, q.y, size.x * 0.5, size.y * 0.5, 0.0);
    if (r.x > 0.0 && p.x < bounds.x + r.x && p.y < bounds.y + r.x)
        result = farther_sdf_sample(result, circle_sdf_sample(p.x, p.y, bounds.x + r.x, bounds.y + r.x, r.x));
    if (r.y > 0.0 && p.x > bounds.z - r.y && p.y < bounds.y + r.y)
        result = farther_sdf_sample(result, circle_sdf_sample(p.x, p.y, bounds.z - r.y, bounds.y + r.y, r.y));
    if (r.z > 0.0 && p.x < bounds.x + r.z && p.y > bounds.w - r.z)
        result = farther_sdf_sample(result, circle_sdf_sample(p.x, p.y, bounds.x + r.z, bounds.w - r.z, r.z));
    if (r.w > 0.0 && p.x > bounds.z - r.w && p.y > bounds.w - r.w)
        result = farther_sdf_sample(result, circle_sdf_sample(p.x, p.y, bounds.z - r.w, bounds.w - r.w, r.w));
    return result;
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
    if (max(max(top_left, top_right), max(bottom_left, bottom_right)) > min(hx, hy)) {
        return extended_rect_sdf_sample(float2(x, y), float4(x0, y0, x1, y1),
            float4(top_left, top_right, bottom_left, bottom_right));
    }
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
