#ifndef TILEINK_HLSL_SDF_POLYGON_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_POLYGON_HLSLI_INCLUDED
#include "base.hlsli"
#include "basic.hlsli"
#include "types.hlsli"

SdfSample distance_to_segment_sdf_sample(float px, float py, float ax, float ay, float bx, float by, float2 fallback_normal) {
    float dx = bx - ax;
    float dy = by - ay;
    float len2 = dx * dx + dy * dy;
    float2 nearest = float2(ax, ay);
    if (len2 > 0.000001) {
        float t = clamp(((px - ax) * dx + (py - ay) * dy) / len2, 0.0, 1.0);
        nearest = float2(ax + dx * t, ay + dy * t);
    }
    float2 delta = float2(px, py) - nearest;
    return make_sdf_sample(length(delta), normalized_or(delta, fallback_normal));
}

float cross_2d(float2 a, float2 b) {
    return a.x * b.y - a.y * b.x;
}

SdfSample triangle_sdf_sample(float x, float y, float ax, float ay, float bx, float by, float cx, float cy, float corner_radius) {
    float2 p = float2(x, y);
    float2 a = float2(ax, ay);
    float2 b = float2(bx, by);
    float2 c = float2(cx, cy);
    float2 ab = b - a;
    float2 bc = c - b;
    float2 ca = a - c;
    float orientation = ((cross_2d(ab, c - a) >= 0.0) ? (1.0) : (-1.0));
    SdfSample sdf_sample = nearer_sdf_sample(
        distance_to_segment_sdf_sample(
            x,
            y,
            ax,
            ay,
            bx,
            by,
            normalized_or(float2(ab.y, -ab.x) * orientation, float2(1.0, 0.0))),
        distance_to_segment_sdf_sample(
            x,
            y,
            bx,
            by,
            cx,
            cy,
            normalized_or(float2(bc.y, -bc.x) * orientation, float2(1.0, 0.0))));
    sdf_sample = nearer_sdf_sample(
        sdf_sample,
        distance_to_segment_sdf_sample(
            x,
            y,
            cx,
            cy,
            ax,
            ay,
            normalized_or(float2(ca.y, -ca.x) * orientation, float2(1.0, 0.0))));
    float side_ab = cross_2d(ab, p - a) * orientation;
    float side_bc = cross_2d(bc, p - b) * orientation;
    float side_ca = cross_2d(ca, p - c) * orientation;
    if (side_ab >= 0.0 && side_bc >= 0.0 && side_ca >= 0.0) {
        sdf_sample = make_sdf_sample(-sdf_sample.distance, -sdf_sample.normal);
    }
    return make_sdf_sample(sdf_sample.distance - max(corner_radius, 0.0), sdf_sample.normal);
}

SdfSample callout_sdf_sample(float x, float y, float x0_raw, float y0_raw, float x1_raw, float y1_raw, float body_radius, float tail_offset, float tail_width, float tail_length, float tail_radius, float side, float tail_visible) {
    float x0 = min(x0_raw, x1_raw);
    float y0 = min(y0_raw, y1_raw);
    float x1 = max(x0_raw, x1_raw);
    float y1 = max(y0_raw, y1_raw);
    SdfSample body = rect_sdf_sample(
        x,
        y,
        x0,
        y0,
        x1,
        y1,
        body_radius,
        body_radius,
        body_radius,
        body_radius);
    if (tail_visible < 0.5) {
        return body;
    }
    bool vertical = side < 0.5 || (side >= 1.5 && side < 2.5);
    float span = ((vertical) ? (x1 - x0) : (y1 - y0));
    float width = min(max(tail_width, 0.0), span);
    float half_width = width * 0.5;
    float center = clamp(tail_offset, half_width, max(span - half_width, half_width));
    float radius = clamp(
        tail_radius,
        0.0,
        min(max(half_width * 0.499, 0.0), max(tail_length * 0.499, 0.0)));
    float source_half_width = max(half_width - radius, 0.0001);
    float source_length = max(tail_length - radius, 0.0001);
    float2 a = float2(x0 + center - source_half_width, y0);
    float2 b = float2(x0 + center + source_half_width, y0);
    float2 c = float2(x0 + center, y0 - source_length);
    if (side >= 0.5 && side < 1.5) {
        a = float2(x1, y0 + center - source_half_width);
        b = float2(x1, y0 + center + source_half_width);
        c = float2(x1 + source_length, y0 + center);
    } else if (side >= 1.5 && side < 2.5) {
        a = float2(x0 + center - source_half_width, y1);
        b = float2(x0 + center + source_half_width, y1);
        c = float2(x0 + center, y1 + source_length);
    } else if (side >= 2.5) {
        a = float2(x0, y0 + center - source_half_width);
        b = float2(x0, y0 + center + source_half_width);
        c = float2(x0 - source_length, y0 + center);
    }
    SdfSample tail = triangle_sdf_sample(x, y, a.x, a.y, b.x, b.y, c.x, c.y, radius);
    return nearer_sdf_sample(body, tail);
}

SdfSample callout_stroke_sdf_sample(float x, float y, float x0, float y0, float x1, float y1, float body_radius, float tail_offset, float tail_width, float tail_length, float tail_radius, float side, float half_width, float tail_visible) {
    SdfSample callout = callout_sdf_sample(
        x,
        y,
        x0,
        y0,
        x1,
        y1,
        body_radius,
        tail_offset,
        tail_width,
        tail_length,
        tail_radius,
        side,
        tail_visible);
    return make_sdf_sample(
        abs(callout.distance) - max(half_width, 0.0),
        ((callout.distance >= 0.0) ? (callout.normal) : (-callout.normal)));
}

float2 star_vertex(uint index, float outer_radius, float inner_radius) {
    float radius = (((index & 1u) == 0u) ? (outer_radius) : (inner_radius));
    float angle = float(index) * 0.6283185307179586;
    return float2(cos(angle), sin(angle)) * radius;
}

SdfSample star_sdf_sample(float x, float y, float center_x, float center_y, float outer_radius, float inner_radius, float corner_radius, float rotation_radians) {
    float cosine = cos(rotation_radians);
    float sine = sin(rotation_radians);
    float2 relative = float2(x - center_x, y - center_y);
    float2 sample_point = float2(
        cosine * relative.x + sine * relative.y,
        -sine * relative.x + cosine * relative.y);
    SdfSample nearest = make_sdf_sample(1.0e20, float2(1.0, 0.0));
    bool inside = false;
    float2 previous = star_vertex(9u, outer_radius, inner_radius);
    for (uint index = 0u; index < 10u; index = index + 1u) {
        float2 current = star_vertex(index, outer_radius, inner_radius);
        float2 edge = current - previous;
        nearest = nearer_sdf_sample(
            nearest,
            distance_to_segment_sdf_sample(
                sample_point.x,
                sample_point.y,
                previous.x,
                previous.y,
                current.x,
                current.y,
                normalized_or(float2(edge.y, -edge.x), float2(1.0, 0.0))));
        if ((previous.y > sample_point.y) != (current.y > sample_point.y)) {
            float crossing_x = previous.x
                + (sample_point.y - previous.y) * (current.x - previous.x)
                    / (current.y - previous.y);
            if (sample_point.x < crossing_x) {
                inside = !inside;
            }
        }
        previous = current;
    }
    if (inside) {
        nearest = make_sdf_sample(-nearest.distance, -nearest.normal);
    }
    SdfSample local = make_sdf_sample(nearest.distance - max(corner_radius, 0.0), nearest.normal);
    return make_sdf_sample(
        local.distance,
        float2(
            cosine * local.normal.x - sine * local.normal.y,
            sine * local.normal.x + cosine * local.normal.y));
}

SdfSample star_stroke_sdf_sample(float x, float y, float center_x, float center_y, float outer_radius, float inner_radius, float corner_radius, float rotation_radians, float half_width) {
    SdfSample star = star_sdf_sample(
        x,
        y,
        center_x,
        center_y,
        outer_radius,
        inner_radius,
        corner_radius,
        rotation_radians);
    return make_sdf_sample(
        abs(star.distance) - max(half_width, 0.0),
        ((star.distance >= 0.0) ? (star.normal) : (-star.normal)));
}

#endif // TILEINK_HLSL_SDF_POLYGON_HLSLI_INCLUDED
