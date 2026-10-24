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
SdfSample rect_sample(float2 p, float4 rect, float4 radii) {
    float2 lower = min(rect.xy, rect.zw), upper = max(rect.xy, rect.zw);
    float2 half_size = (upper - lower) * 0.5f, relative = p - (lower + upper) * 0.5f;
    float radius = relative.x >= 0 ? (relative.y <= 0 ? radii.y : radii.w) : (relative.y > 0 ? radii.z : radii.x);
    return rounded_box(relative, half_size, max(min(min(radius, half_size.x), half_size.y), 0.0f));
}
SdfSample local_line_rect(float axis, float normal, float start, float end, float half_height) {
    return rounded_box(float2(axis - (start + end) * 0.5f, normal), float2((end - start) * 0.5f, half_height), 0);
}
float euclidean_remainder(float value, float modulus) { return fma(-floor(value / modulus), modulus, value); }
