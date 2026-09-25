#ifndef TILEINK_HLSL_LAYER_GEOMETRY_HLSLI_INCLUDED
#define TILEINK_HLSL_LAYER_GEOMETRY_HLSLI_INCLUDED
#include "../constants.hlsli"
#include "../draw_records.hlsli"
#include "../scene_records.hlsli"
#include "affine.hlsli"
#include "draw_tags.hlsli"
#include "pixel.hlsli"
#include "draw.hlsli"
#include "coverage.hlsli"
#include "sdf/coverage.hlsli"

// Geometry resources are explicit; allocations never substitute for scene references.
uint layer_alpha_at(ByteAddressBuffer draws, ByteAddressBuffer paths,
    ByteAddressBuffer backdrops, ByteAddressBuffer ranges, ByteAddressBuffer segments,
    ByteAddressBuffer paint, uint draw_index, uint shadow_base, uint2 xy, uint2 tile_dimensions) {
    uint bytes; draws.GetDimensions(bytes);
    if (draw_index>=bytes/DRAW_RECORD_STRIDE) return 0u;
    DrawData draw=load_draw(draws,draw_index);
    if (draw_has_sdf(draw)) {
        if (any(int2(xy)<draw.pixel_bounds.xy) || any(int2(xy)>=draw.pixel_bounds.zw)) return 0u;
        uint transform_address=draw_index*DRAW_RECORD_STRIDE+DRAW_INVERSE_TRANSFORM;
        AffineRecord inverse_transform=load_affine(draws,transform_address);
        float2 local=affine_record_point(inverse_transform,float2(xy)+0.5);
        bool shadow=draw.sdf_offset==INVALID_INDEX;
        uint offset=shadow?shadow_base+draw.shadow_offset:draw.sdf_offset;
        return coverage_to_u8(sdf_coverage_from_blob(paint,offset,local.x,local.y,inverse_transform));
    }
    uint2 tile=xy/TILE_SIZE;
    uint index=draw_backdrop(paths,draw,tile,tile_dimensions);
    if (index==INVALID_INDEX) return 0u;
    uint2 range=ranges.Load2(index*TILE_SEGMENT_RANGE_STRIDE);
    return fill_alpha_at(segments,asint(backdrops.Load(index*4u)),draw.fill_rule,
        range.x,range.y,xy.x%TILE_SIZE,xy.y%TILE_SIZE);
}
#endif
