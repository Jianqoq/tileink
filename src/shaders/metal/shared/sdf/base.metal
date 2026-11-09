// Keep distance and normal calculations in the same math mode so quantized
// coverage remains stable at half-alpha boundaries.
#include "../affine.metal"
struct SdfSample { float distance; float2 normal; };
// Positive infinity is the identity of a distance union. Fixed "far" values can become the
// nearest surface after magnification, painting empty dash intervals and zero-length butt lines.
SdfSample empty_sdf_sample() { return {as_type<float>(0x7f800000u), float2(1, 0)}; }
// Scaling only when squared length exceeds the float exponent range preserves the ordinary
// path while retaining short/long authored edges. These are range checks, not geometric epsilons.
float sdf_length(float2 value) {
    float magnitude = fast::length(value);
    if (magnitude > 0.0f && isfinite(magnitude)) return magnitude;
    float axis = max(abs(value.x), abs(value.y));
    return axis == 0.0f ? 0.0f : axis * fast::length(value / axis);
}
float2 normalized_or(float2 value, float2 fallback) {
    float magnitude = fast::length(value);
    if (magnitude > 0.0f && isfinite(magnitude)) return value / magnitude;
    float axis = max(abs(value.x), abs(value.y));
    if (axis == 0.0f) return fallback;
    float2 scaled = value / axis;
    return scaled / fast::length(scaled);
}
SdfSample nearer(SdfSample a, SdfSample b) { return a.distance <= b.distance ? a : b; }
SdfSample farther(SdfSample a, SdfSample b) { return a.distance >= b.distance ? a : b; }
float sdf_coverage(SdfSample s, Affine inverse) {
    float2 gradient(fma(inverse.linear.x, s.normal.x, inverse.linear.y * s.normal.y),
        fma(inverse.linear.z, s.normal.x, inverse.linear.w * s.normal.y));
    float footprint = sdf_length(gradient);
    // A truly collapsed footprint is a step; a small nonzero footprint still has its own AA ramp.
    if (footprint == 0.0f) return s.distance < 0.0f ? 1.0f : (s.distance > 0.0f ? 0.0f : 0.5f);
    return clamp(0.5f - s.distance / footprint, 0.0f, 1.0f);
}
float sdf_shadow(SdfSample s, Affine inverse, float expand, float intensity) {
    intensity = clamp(intensity, 0.0f, 1.0f);
    return expand > 0 ? clamp(exp(-max(s.distance, 0.0f) / expand) * intensity, 0.0f, 1.0f)
        : sdf_coverage(s, inverse) * intensity;
}
SdfSample rounded_box(float2 p, float2 half_size, float radius) {
    float2 q = abs(p) - half_size + radius, outside = max(q, float2(0));
    float outside_length = sdf_length(outside);
    float distance = min(max(q.x, q.y), 0.0f) + outside_length - radius;
    float2 signs(p.x >= 0 ? 1 : -1, p.y >= 0 ? 1 : -1);
    if (outside_length > 0.0f) return {distance, outside / outside_length * signs};
    return {distance, q.x > q.y ? float2(signs.x, 0) : float2(0, signs.y)};
}
SdfSample circle_sample(float2 p, float2 center, float radius) {
    float2 delta = p - center;
    return {sdf_length(delta) - radius, normalized_or(delta, float2(1, 0))};
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
        float4 scaled = radii * 5.42101086242752217e-20;
        float divisor = largest * 5.42101086242752217e-20;
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


SdfSample extended_rect_sample(float2 p, float4 bounds, float4 radii) {
    float2 size = bounds.zw - bounds.xy;
    float2 center = (bounds.xy + bounds.zw) * 0.5f;
    float2 q = p - center;
    float4 r = css_rect_radii(radii, size);
    float largest = max(max(r.x, r.y), max(r.z, r.w));
    if (largest <= min(size.x, size.y) * 0.5f) {
        float radius = q.x >= 0.0f ? (q.y <= 0.0f ? r.y : r.w) : (q.y > 0.0f ? r.z : r.x);
        return rounded_box(q, size * 0.5f, radius);
    }
    SdfSample result = rounded_box(q, size * 0.5f, 0.0f);
    if (r.x > 0.0f && p.x < bounds.x + r.x && p.y < bounds.y + r.x)
        result = farther(result, circle_sample(p, float2(bounds.x + r.x, bounds.y + r.x), r.x));
    if (r.y > 0.0f && p.x > bounds.z - r.y && p.y < bounds.y + r.y)
        result = farther(result, circle_sample(p, float2(bounds.z - r.y, bounds.y + r.y), r.y));
    if (r.z > 0.0f && p.x < bounds.x + r.z && p.y > bounds.w - r.z)
        result = farther(result, circle_sample(p, float2(bounds.x + r.z, bounds.w - r.z), r.z));
    if (r.w > 0.0f && p.x > bounds.z - r.w && p.y > bounds.w - r.w)
        result = farther(result, circle_sample(p, float2(bounds.z - r.w, bounds.w - r.w), r.w));
    return result;
}

SdfSample rect_sample(float2 p, float4 rect, float4 radii) {
    float2 lower = min(rect.xy, rect.zw), upper = max(rect.xy, rect.zw);
    float2 half_size = (upper - lower) * 0.5f, relative = p - (lower + upper) * 0.5f;
    if (max(max(radii.x, radii.y), max(radii.z, radii.w)) > min(half_size.x, half_size.y))
        return extended_rect_sample(p, float4(lower, upper), radii);
    float radius = relative.x >= 0 ? (relative.y <= 0 ? radii.y : radii.w) : (relative.y > 0 ? radii.z : radii.x);
    return rounded_box(relative, half_size, max(min(min(radius, half_size.x), half_size.y), 0.0f));
}
SdfSample local_line_rect(float axis, float normal, float start, float end, float half_height) {
    return rounded_box(float2(axis - (start + end) * 0.5f, normal), float2((end - start) * 0.5f, half_height), 0);
}
float euclidean_remainder(float value, float modulus) { return fma(-floor(value / modulus), modulus, value); }
