// Use the explicit geometric functions selected by the WGSL Metal runtime.
// The offline compiler also preserves invariance under the runtime math mode.
// Mixing safe-mode compilation with that runtime shifts half-alpha boundaries.
#include "../affine.metal"
struct SdfSample { float distance; float2 normal; };
float2 normalized_or(float2 value, float2 fallback) {
    float magnitude = fast::length(value);
    return magnitude > 0.000001f ? value / magnitude : fallback;
}
SdfSample nearer(SdfSample a, SdfSample b) { return a.distance <= b.distance ? a : b; }
SdfSample farther(SdfSample a, SdfSample b) { return a.distance >= b.distance ? a : b; }
float sdf_coverage(SdfSample s, Affine inverse) {
    float2 gradient(fma(inverse.linear.x, s.normal.x, inverse.linear.y * s.normal.y),
        fma(inverse.linear.z, s.normal.x, inverse.linear.w * s.normal.y));
    return clamp(0.5f - s.distance / max(fast::length(gradient), 0.000001f), 0.0f, 1.0f);
}
float sdf_shadow(SdfSample s, Affine inverse, float expand, float intensity) {
    intensity = clamp(intensity, 0.0f, 1.0f);
    return expand > 0 ? clamp(exp(-max(s.distance, 0.0f) / expand) * intensity, 0.0f, 1.0f)
        : sdf_coverage(s, inverse) * intensity;
}
SdfSample rounded_box(float2 p, float2 half_size, float radius) {
    float2 q = abs(p) - half_size + radius, outside = max(q, float2(0));
    float distance = min(max(q.x, q.y), 0.0f) + fast::length(outside) - radius;
    float2 signs(p.x >= 0 ? 1 : -1, p.y >= 0 ? 1 : -1);
    if (dot(outside, outside) > 1.0e-12f) return {distance, fast::normalize(outside) * signs};
    return {distance, q.x > q.y ? float2(signs.x, 0) : float2(0, signs.y)};
}
SdfSample circle_sample(float2 p, float2 center, float radius) {
    float2 delta = p - center;
    return {fast::length(delta) - radius, normalized_or(delta, float2(1, 0))};
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
