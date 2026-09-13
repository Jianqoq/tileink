#ifndef TILEINK_HLSL_COARSE_SDF_CLIP_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_SDF_CLIP_HLSLI_INCLUDED

#include "draw.hlsli"

static const float FULL_TILE_SDF_INSET = 0.5;
bool rounded_corner_covers(float2 position, float2 corner, float2 center, float radius) {
    bool in_square = radius > 0.0 && all(abs(position - corner) < radius);
    float inner_radius = radius - FULL_TILE_SDF_INSET;
    float2 delta = position - center;
    return !in_square || (inner_radius > 0.0 && dot(delta, delta) <= inner_radius * inner_radius);
}
bool sdf_clip_covers(ByteAddressBuffer sdf, CoarseDraw draw, uint2 tile) {
    if (draw.sdf_offset == INVALID_INDEX || draw.shadow_offset != INVALID_INDEX || draw.sdf_len < 9u) return false;
    uint base = draw.sdf_offset * 4u;
    if (sdf.Load(base) != SDF_RECT || any(draw.affine_linear != float4(1.0, 0.0, 0.0, 1.0))) return false;
    float4 rect = asfloat(sdf.Load4(base + 4u));
    float2 lower = min(rect.xy, rect.zw);
    float2 upper = max(rect.xy, rect.zw);
    float2 tile_lower = float2(tile * TILE_SIZE) + 0.5 - draw.translation;
    float2 tile_upper = tile_lower + float(TILE_SIZE - 1u);
    float2 size = upper - lower;
    if (any(size < float(TILE_SIZE - 1u) + 2.0 * FULL_TILE_SDF_INSET) || any(tile_lower < lower + FULL_TILE_SDF_INSET) || any(tile_upper > upper - FULL_TILE_SDF_INSET)) return false;
    float limit = min(size.x, size.y) * 0.5;
    float4 radii = min(max(asfloat(sdf.Load4(base + 20u)), 0.0), limit);
    return rounded_corner_covers(tile_lower, lower, lower + radii.x, radii.x)
        && rounded_corner_covers(float2(tile_upper.x, tile_lower.y), float2(upper.x, lower.y), float2(upper.x-radii.y, lower.y+radii.y), radii.y)
        && rounded_corner_covers(float2(tile_lower.x, tile_upper.y), float2(lower.x, upper.y), float2(lower.x+radii.z, upper.y-radii.z), radii.z)
        && rounded_corner_covers(tile_upper, upper, upper-radii.w, radii.w);
}

#endif // TILEINK_HLSL_COARSE_SDF_CLIP_HLSLI_INCLUDED
