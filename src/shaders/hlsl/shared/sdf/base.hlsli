#ifndef TILEINK_HLSL_SDF_BASE_HLSLI_INCLUDED
#define TILEINK_HLSL_SDF_BASE_HLSLI_INCLUDED
#include "types.hlsli"

float2 normalized_or(float2 value, float2 fallback) {
    float magnitude = length(value);
    if (magnitude > 0.000001) {
        return value / magnitude;
    }
    return fallback;
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

float sdf_device_distance(SdfSample sdf_sample, AffineRecord inverse_transform) {
    float2 device_gradient = float2(
        mad(inverse_transform.a,sdf_sample.normal.x,inverse_transform.b*sdf_sample.normal.y),
        mad(inverse_transform.c,sdf_sample.normal.x,inverse_transform.d*sdf_sample.normal.y));
    return sdf_sample.distance / max(length(device_gradient), 0.000001);
}

float sdf_coverage_from_dist(float dist) {
    return clamp(0.5 - dist, 0.0, 1.0);
}

float sdf_coverage_from_sample(SdfSample sdf_sample, AffineRecord inverse_transform) {
    return sdf_coverage_from_dist(sdf_device_distance(sdf_sample, inverse_transform));
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
