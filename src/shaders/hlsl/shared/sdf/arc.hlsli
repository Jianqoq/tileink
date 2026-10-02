#ifndef TILEINK_HLSL_SDF_ARC_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_ARC_HLSLI_INCLUDED
#include "base.hlsli"
#include "basic.hlsli"
#include "line.hlsli"
#include "polygon.hlsli"
#include "types.hlsli"
#include "../pixel.hlsli"

bool arc_angle_in_sweep(float angle, float start_angle, float sweep_angle) {
    float tau = 6.2831855;
    float eps = 0.000001;
    if (sweep_angle >= 0.0) {
        return rem_euclid_f32(angle - start_angle, tau) <= sweep_angle + eps;
    }
    return rem_euclid_f32(start_angle - angle, tau) <= -sweep_angle + eps;
}

SdfSample arc_cap_segment_sdf_sample(float vx, float vy, float radius, float angle, float half_extent, float sweep_angle, bool end_cap) {
    float inner_radius = max(radius - half_extent, 0.0);
    float outer_radius = radius + half_extent;
    float co = cos(angle);
    float si = sin(angle);
    float direction = ((sweep_angle >= 0.0) ? (1.0) : (-1.0));
    float2 tangent = float2(-si * direction, co * direction);
    float2 fallback = ((end_cap) ? (tangent) : (-tangent));
    return distance_to_segment_sdf_sample(
        vx,
        vy,
        inner_radius * co,
        inner_radius * si,
        outer_radius * co,
        outer_radius * si,
        fallback);
}

SdfSample arc_endpoint_sdf_sample(float vx, float vy, float radius, float angle, float half_extent) {
    float ex = radius * cos(angle);
    float ey = radius * sin(angle);
    float2 delta = float2(vx - ex, vy - ey);
    return make_sdf_sample(length(delta) - half_extent, normalized_or(delta, float2(cos(angle), sin(angle))));
}

SdfSample arc_butt_sdf_sample(float vx, float vy, float len, float radius, float half_extent, float start_angle, float sweep_angle) {
    if (len <= 0.000001) {
        return nearer_sdf_sample(
            arc_endpoint_sdf_sample(vx, vy, radius, start_angle, half_extent),
            arc_endpoint_sdf_sample(vx, vy, radius, start_angle + sweep_angle, half_extent));
    }
    float angle = atan2(vy, vx);
    float radial = abs(len - radius) - half_extent;
    if (arc_angle_in_sweep(angle, start_angle, sweep_angle)) {
        float2 normal = float2(vx, vy) / len * ((len >= radius) ? (1.0) : (-1.0));
        return make_sdf_sample(radial, normal);
    }
    return nearer_sdf_sample(
        arc_cap_segment_sdf_sample(vx, vy, radius, start_angle, half_extent, sweep_angle, false),
        arc_cap_segment_sdf_sample(
            vx,
            vy,
            radius,
            start_angle + sweep_angle,
            half_extent,
            sweep_angle,
            true));
}

SdfSample arc_square_cap_sdf_sample(float vx, float vy, float radius, float angle, float sweep_angle, float x0, float x1, float half_extent) {
    float dir = 1.0;
    if (sweep_angle < 0.0) {
        dir = -1.0;
    }
    float co = cos(angle);
    float si = sin(angle);
    float ex = radius * co;
    float ey = radius * si;
    float tangent_x = -si * dir;
    float tangent_y = co * dir;
    float px = vx - ex;
    float py = vy - ey;
    float local_x = px * tangent_x + py * tangent_y;
    float local_y = -px * tangent_y + py * tangent_x;
    return line_local_to_canvas_sample(
        local_line_rect_sample(local_x, local_y, x0, x1, half_extent),
        tangent_x,
        tangent_y);
}

SdfSample arc_sdf_sample(float x, float y, float cx, float cy, float radius_raw, float width_raw, float start_angle, float sweep_angle, float cap) {
    float radius = max(radius_raw, 0.0);
    float width = max(width_raw, 0.0);
    SdfSample sdf_sample = make_sdf_sample(1000000.0, float2(1.0, 0.0));
    if (radius > 0.000001 && width > 0.000001 && abs(sweep_angle) > 0.000001) {
        float vx = x - cx;
        float vy = y - cy;
        float len = sqrt(vx * vx + vy * vy);
        float half_extent = width * 0.5;
        if (abs(sweep_angle) >= 6.2830853) {
            float2 radial_normal = normalized_or(float2(vx, vy), float2(1.0, 0.0));
            sdf_sample = make_sdf_sample(abs(len - radius) - half_extent, radial_normal * ((len >= radius) ? (1.0) : (-1.0)));
        } else {
            sdf_sample = arc_butt_sdf_sample(vx, vy, len, radius, half_extent, start_angle, sweep_angle);
            if (cap > 1.5) {
                sdf_sample = nearer_sdf_sample(
                    nearer_sdf_sample(sdf_sample, arc_endpoint_sdf_sample(vx, vy, radius, start_angle, half_extent)),
                    arc_endpoint_sdf_sample(vx, vy, radius, start_angle + sweep_angle, half_extent));
            } else if (cap >= 0.5) {
                sdf_sample = nearer_sdf_sample(
                    nearer_sdf_sample(
                        sdf_sample,
                        arc_square_cap_sdf_sample(vx, vy, radius, start_angle, sweep_angle, -half_extent, 0.0, half_extent)),
                    arc_square_cap_sdf_sample(
                        vx,
                        vy,
                        radius,
                        start_angle + sweep_angle,
                        sweep_angle,
                        0.0,
                        half_extent,
                        half_extent));
            }
        }
    }
    return sdf_sample;
}

#endif // TILEINK_HLSL_SDF_ARC_HLSLI_INCLUDED
