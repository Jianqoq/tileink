#ifndef TILEINK_HLSL_SDF_BASE_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_BASE_HLSLI_INCLUDED
#include "types.hlsli"
// Distance unions require an unbounded identity, independent of authored coordinate scale.
SdfSample empty_sdf_sample() { return make_sdf_sample(asfloat(0x7f800000u), float2(1.0,0.0)); }

// Preserve ordinary arithmetic; scale only when squared length exceeds the float exponent range.
float sdf_length(float2 value) {
    float magnitude = length(value);
    if (magnitude > 0.0 && isfinite(magnitude)) return magnitude;
    float axis = max(abs(value.x), abs(value.y));
    return axis == 0.0 ? 0.0 : axis * length(value / axis);
}

float2 normalized_or(float2 value, float2 fallback) {
    float magnitude = length(value);
    if (magnitude > 0.0 && isfinite(magnitude)) {
        return value / magnitude;
    }
    float axis = max(abs(value.x), abs(value.y));
    if (axis == 0.0) return fallback;
    float2 scaled = value / axis;
    return scaled / length(scaled);
}

SdfSample nearer_sdf_sample(SdfSample a, SdfSample b) {
    if (a.distance <= b.distance) {
        return a;
    }
    return b;
}

SdfSample farther_sdf_sample(SdfSample a, SdfSample b) {
    if (a.distance >= b.distance) {
        return a;
    }
    return b;
}

float sdf_coverage_from_dist(float dist) {
    return clamp(0.5 - dist, 0.0, 1.0);
}

float sdf_coverage_from_sample(SdfSample sdf_sample, AffineRecord inverse_transform) {
    float2 gradient = float2(
        mad(inverse_transform.a,sdf_sample.normal.x,inverse_transform.b*sdf_sample.normal.y),
        mad(inverse_transform.c,sdf_sample.normal.x,inverse_transform.d*sdf_sample.normal.y));
    float footprint = sdf_length(gradient);
    // Zero has step coverage; a small nonzero footprint retains its actual antialiasing ramp.
    if (footprint == 0.0) return sdf_sample.distance < 0.0 ? 1.0 : (sdf_sample.distance > 0.0 ? 0.0 : 0.5);
    return sdf_coverage_from_dist(sdf_sample.distance / footprint);
}

float sdf_shadow_coverage_from_sample(SdfSample sdf_sample, AffineRecord inverse_transform, float expand, float intensity) {
    float clamped_intensity = clamp(intensity, 0.0, 1.0);
    if (expand > 0.0) {
        // Shadow expansion belongs to the local geometry and must transform with it. Only the
        // fixed antialiasing ramp below is defined in device pixels.
        return clamp(exp(-max(sdf_sample.distance, 0.0) / expand) * clamped_intensity, 0.0, 1.0);
    }
    return sdf_coverage_from_sample(sdf_sample, inverse_transform) * clamped_intensity;
}

#endif // TILEINK_HLSL_SDF_BASE_HLSLI_INCLUDED
