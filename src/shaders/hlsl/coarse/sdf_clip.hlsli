#ifndef TILEINK_HLSL_COARSE_SDF_CLIP_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_SDF_CLIP_HLSLI_INCLUDED

#include "tags.hlsli"
#include "../shared/draw.hlsli"
#include "../shared/sdf/constants.hlsli"

static const float FULL_TILE_SDF_INSET = 0.5;
bool rounded_corner_covers(float2 position, float2 corner, float2 center, float radius, float inset) {
    bool in_square = radius > 0.0 && all(abs(position - corner) < radius);
    float inner_radius = radius - inset;
    float2 delta = position - center;
    return !in_square || (inner_radius > 0.0 && dot(delta, delta) <= inner_radius * inner_radius);
}
bool rect_covers_tile(float4 rect, float4 radii, DrawData draw, uint2 tile, float inset) {
    float2 lower = min(rect.xy, rect.zw);
    float2 upper = max(rect.xy, rect.zw);
    float2 tile_lower = float2(tile * TILE_SIZE) + 0.5 - draw.translation;
    float2 tile_upper = tile_lower + float(TILE_SIZE - 1u);
    float2 size = upper - lower;
    if (any(size < float(TILE_SIZE - 1u) + 2.0 * inset) || any(tile_lower < lower + inset) || any(tile_upper > upper - inset)) return false;
    float limit = min(size.x, size.y) * 0.5;
    // Large finite coordinates can lose the AA inset or overflow fine's center.
    // Restrict this full-tile proof; uncertain geometry keeps exact fine evaluation.
    if (!all(abs(rect) <= 65536.0) || !all(abs(radii) <= 65536.0)
        || !all(abs(draw.translation) <= 65536.0)) return false;
    radii = min(max(radii, 0.0), limit);
    return rounded_corner_covers(tile_lower, lower, lower + radii.x, radii.x, inset)
        && rounded_corner_covers(float2(tile_upper.x, tile_lower.y), float2(upper.x, lower.y), float2(upper.x-radii.y, lower.y+radii.y), radii.y, inset)
        && rounded_corner_covers(float2(tile_lower.x, tile_upper.y), float2(lower.x, upper.y), float2(lower.x+radii.z, upper.y-radii.z), radii.z, inset)
        && rounded_corner_covers(tile_upper, upper, upper-radii.w, radii.w, inset);
}

bool sdf_clip_covers(ByteAddressBuffer sdf, DrawData draw, uint2 tile) {
    if (draw.sdf_offset == INVALID_INDEX || draw.shadow_offset != INVALID_INDEX || draw.sdf_len < 9u) return false;
    uint base = draw.sdf_offset * 4u;
    if (sdf.Load(base) != SDF_RECT || any(draw.affine_linear != float4(1.0, 0.0, 0.0, 1.0))) return false;
    return rect_covers_tile(asfloat(sdf.Load4(base + 4u)), asfloat(sdf.Load4(base + 20u)), draw, tile, FULL_TILE_SDF_INSET);
}
bool sdf_stroke_empty(ByteAddressBuffer sdf, DrawData draw, uint2 tile) {
    if (draw.sdf_offset == INVALID_INDEX || draw.shadow_offset != INVALID_INDEX || draw.sdf_len < 13u
        || any(draw.affine_linear != float4(1.0, 0.0, 0.0, 1.0))) return false;
    uint base = draw.sdf_offset * 4u;
    if (sdf.Load(base) != SDF_KIND_RECT_STROKE) return false;
    float4 rect = asfloat(sdf.Load4(base + 4u));
    float4 radii = asfloat(sdf.Load4(base + 20u));
    float4 stroke = asfloat(sdf.Load4(base + 36u));
    if (!all(abs(rect) <= 65536.0) || !all(abs(radii) <= 65536.0)
        || !all(abs(stroke) <= 65536.0)) return false;
    float4 half_width = max(stroke, 0.0);
    float2 lower = min(rect.xy, rect.zw), upper = max(rect.xy, rect.zw);
    float4 inner = float4(lower.x + half_width.w, lower.y + half_width.x,
        upper.x - half_width.y, upper.y - half_width.z);
    if (inner.x >= inner.z || inner.y >= inner.w) return false;
    float4 corner = float4(max(half_width.x, half_width.w), max(half_width.x, half_width.y),
        max(half_width.z, half_width.w), max(half_width.z, half_width.y));
    // Coverage is clamp(outer - inner, 0, 1). Keep a half-pixel rounding reserve.
    return rect_covers_tile(inner, max(radii - corner, 0.0), draw, tile, 1.0);
}

#endif // TILEINK_HLSL_COARSE_SDF_CLIP_HLSLI_INCLUDED
