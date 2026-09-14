#ifndef TILEINK_FILTER_GLASS_GEOMETRY_HLSLI
#define TILEINK_FILTER_GLASS_GEOMETRY_HLSLI
#include "constants.hlsli"
#include "../config.hlsli"
float liquid_glass_edge(float inside_distance, float refraction_thickness, float refraction_factor) {
    float thickness = max(refraction_thickness, LIQUID_GLASS_EPSILON);
    float factor = max(refraction_factor, 1.0);
    if (inside_distance >= thickness || factor == 1.0) {
        return 0.0;
    }

    // Snell's law gives sin(theta_t) directly. Compute tan(theta_i - theta_t)
    // from sine/cosine products: the previous asin/sin/asin/tan chain varied
    // between APIs and moved refracted samples across RGBA8 rounding boundaries.
    // Explicit FMA fixes evaluation order; this removes the numerical root cause,
    // without quantizing coordinates or changing the refraction model.
    float ratio = clamp(1.0 - inside_distance / thickness, 0.0, 1.0);
    float sin_i = ratio * ratio;
    float sin_t = sin_i / factor;
    float cos_t = sqrt(max(mad(-sin_t, sin_t, 1.0), 0.0));
    if (sin_i == 1.0) {
        // At grazing incidence tan(theta_i - theta_t) = factor * cos(theta_t).
        // Avoid dividing by a subnormal sin(theta_t): even a finite factor can
        // otherwise overflow the edge value and turn a zero normal into NaN.
        return factor * cos_t;
    }
    float cos_i = sqrt(max(mad(-sin_i, sin_i, 1.0), 0.0));
    float numerator = mad(sin_i, cos_t, -(cos_i * sin_t));
    float denominator = mad(cos_i, cos_t, sin_i * sin_t);
    return max(numerator / denominator, 0.0);
}

float liquid_glass_highlight_geometry(float distance, float fresnel_range, float fresnel_hardness) {
    // Fifth power is monotonic: clamping the base first preserves the final
    // clamp while avoiding undefined pow(negative, 5) behavior on native APIs.
    float base = 1.0 + distance / LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE *
        pow(LIQUID_GLASS_GEOMETRY_RANGE_SCALE / max(fresnel_range, LIQUID_GLASS_EPSILON), 2.0) + fresnel_hardness;
    return pow(clamp(base, 0.0, 1.0), 5.0);
}



float liquid_glass_vec2_angle(float x, float y) {
    float len = sqrt(x * x + y * y);
    float angle = 0.0;
    if (len >= 0.00000001) {
        angle = atan2(y, x);
        if (angle < 0.0) {
            angle += 2.0 * LIQUID_GLASS_PI;
        }
    }
    return angle;
}

float liquid_glass_glare_angle(ConstantBuffer<FilterConfig> config, float nx, float ny) {
    float angle = (liquid_glass_vec2_angle(nx, ny) - LIQUID_GLASS_PI * 0.25 + config.liquid_glare_angle) * 2.0;
    float side = LIQUID_GLASS_GLARE_SIDE_SCALE;
    if ((angle > LIQUID_GLASS_PI * 1.5 && angle < LIQUID_GLASS_PI * 3.5) || angle < -LIQUID_GLASS_PI * 0.5) {
        side = LIQUID_GLASS_GLARE_SIDE_SCALE * config.liquid_glare_opposite_factor;
    }
    return clamp(
        pow(
            (0.5 + sin(angle) * 0.5) * side * config.liquid_glare_factor,
            LIQUID_GLASS_GLARE_POWER_BASE + config.liquid_glare_convergence * LIQUID_GLASS_GLARE_POWER_SCALE),
        0.0,
        1.0);
}

float liquid_glass_smoothstep(float edge0, float edge1, float x) {
    float t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

float liquid_glass_corner_radius(float px, float py, float radius_top_left, float radius_top_right, float radius_bottom_left, float radius_bottom_right) {
    float radius = radius_top_left;
    if (px >= 0.0) {
        if (py <= 0.0) {
            radius = radius_top_right;
        } else {
            radius = radius_bottom_right;
        }
    } else if (py > 0.0) {
        radius = radius_bottom_left;
    }
    return radius;
}

float liquid_glass_round_rect_distance(float x, float y, float x0, float y0, float x1, float y1, float radius_top_left, float radius_top_right, float radius_bottom_left, float radius_bottom_right) {
    float cx = (x0 + x1) * 0.5;
    float cy = (y0 + y1) * 0.5;
    float hx = max((x1 - x0) * 0.5, 0.0);
    float hy = max((y1 - y0) * 0.5, 0.0);
    float px = x - cx;
    float py = y - cy;
    float radius = max(min(min(liquid_glass_corner_radius(px, py, radius_top_left, radius_top_right, radius_bottom_left, radius_bottom_right), hx), hy), 0.0);
    float ax = abs(px);
    float ay = abs(py);
    float dx = ax - hx;
    float dy = ay - hy;
    float result = sqrt(max(dx, 0.0) * max(dx, 0.0) + max(dy, 0.0) * max(dy, 0.0)) + min(max(dx, dy), 0.0);
    if (radius > 0.0) {
        float qx = ax - hx + radius;
        float qy = ay - hy + radius;
        result = min(max(qx, qy), 0.0) + sqrt(max(qx, 0.0) * max(qx, 0.0) + max(qy, 0.0) * max(qy, 0.0)) - radius;
    }
    return result;
}

float2 liquid_glass_normal(ConstantBuffer<FilterConfig> config, float x, float y) {
    float eps = 1.0;
    float dx = liquid_glass_round_rect_distance(x + eps, y, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right) -
        liquid_glass_round_rect_distance(x - eps, y, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right);
    float dy = liquid_glass_round_rect_distance(x, y + eps, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right) -
        liquid_glass_round_rect_distance(x, y - eps, config.rect_x0, config.rect_y0, config.rect_x1, config.rect_y1, config.radius_top_left, config.radius_top_right, config.radius_bottom_left, config.radius_bottom_right);
    float len = sqrt(dx * dx + dy * dy);
    if (len > LIQUID_GLASS_EPSILON) {
        return float2(dx / len, dy / len);
    }
    return float2(0.0, -1.0);
}
#endif
