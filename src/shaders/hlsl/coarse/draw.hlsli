#ifndef TILEINK_HLSL_COARSE_DRAW_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_DRAW_HLSLI_INCLUDED

#include "../constants.hlsli"
#include "../draw_records.hlsli"
#include "../scene_records.hlsli"
#include "tags.hlsli"

struct CoarseDraw {
    uint path_id, glyph_run_id, sdf_offset, sdf_len, shadow_offset, tag, fill_rule;
    int4 pixel_bounds;
    float4 affine_linear;
    float2 translation;
};
CoarseDraw load_draw(ByteAddressBuffer records, uint index) {
    uint base = index * DRAW_RECORD_STRIDE;
    CoarseDraw draw;
    draw.path_id = records.Load(base + DRAW_PATH);
    draw.glyph_run_id = records.Load(base + DRAW_GLYPH_RUN);
    draw.sdf_offset = records.Load(base + DRAW_SDF);
    draw.sdf_len = records.Load(base + DRAW_SDF_LEN);
    draw.shadow_offset = records.Load(base + DRAW_SHADOW);
    draw.tag = records.Load(base + DRAW_TAG);
    draw.fill_rule = records.Load(base + DRAW_FILL_RULE);
    draw.pixel_bounds = asint(records.Load4(base + DRAW_PIXEL_BOUNDS));
    draw.affine_linear = asfloat(records.Load4(base + DRAW_TRANSFORM));
    draw.translation = asfloat(records.Load2(base + DRAW_TRANSFORM + AFFINE_TRANSLATION));
    return draw;
}
bool draw_has_sdf(CoarseDraw draw) { return draw.sdf_offset != INVALID_INDEX || draw.shadow_offset != INVALID_INDEX; }
uint tile_min(int pixel, uint limit) { return pixel > 0 ? min(uint(pixel) / TILE_SIZE, limit) : 0u; }
uint tile_max(int pixel, uint limit) { return pixel > 0 ? min((uint(pixel) + TILE_SIZE - 1u) / TILE_SIZE, limit) : 0u; }
bool draw_hits_tile(CoarseDraw draw, uint2 tile, uint2 dimensions) {
    uint2 lower = uint2(tile_min(draw.pixel_bounds.x, dimensions.x), tile_min(draw.pixel_bounds.y, dimensions.y));
    uint2 upper = uint2(tile_max(draw.pixel_bounds.z, dimensions.x), tile_max(draw.pixel_bounds.w, dimensions.y));
    return all(tile >= lower) && all(tile < upper);
}
uint draw_backdrop(ByteAddressBuffer paths, CoarseDraw draw, uint2 tile, uint2 dimensions) {
    uint path_bytes; paths.GetDimensions(path_bytes);
    if (draw.path_id == INVALID_INDEX || draw.tag > DRAW_PATH_GLYPH || draw.path_id >= path_bytes / PATH_RECORD_STRIDE || !draw_hits_tile(draw, tile, dimensions)) return INVALID_INDEX;
    uint base = draw.path_id * PATH_RECORD_STRIDE;
    uint4 bbox = paths.Load4(base + PATH_BBOX);
    if (bbox.z <= bbox.x || any(tile < bbox.xy) || any(tile >= bbox.zw)) return INVALID_INDEX;
    return paths.Load(base + PATH_DATA_OFFSET) + (tile.y - bbox.y) * (bbox.z - bbox.x) + tile.x - bbox.x;
}
float2 draw_point(CoarseDraw draw, float2 position) {
    return float2(draw.affine_linear.x * position.x + draw.affine_linear.z * position.y + draw.translation.x,
                  draw.affine_linear.y * position.x + draw.affine_linear.w * position.y + draw.translation.y);
}
bool transformed_rect_hits_tile(CoarseDraw draw, int4 rect, uint2 tile) {
    float2 p0 = draw_point(draw, float2(rect.xy));
    float2 p1 = draw_point(draw, float2(rect.zy));
    float2 p2 = draw_point(draw, float2(rect.xw));
    float2 p3 = draw_point(draw, float2(rect.zw));
    float2 lower = min(min(p0, p1), min(p2, p3));
    float2 upper = max(max(p0, p1), max(p2, p3));
    float2 tile_lower = float2(tile * TILE_SIZE);
    return all(lower < tile_lower + float(TILE_SIZE)) && all(upper > tile_lower);
}

#endif // TILEINK_HLSL_COARSE_DRAW_HLSLI_INCLUDED
