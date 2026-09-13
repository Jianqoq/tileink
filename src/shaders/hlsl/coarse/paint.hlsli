#ifndef TILEINK_HLSL_COARSE_PAINT_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_PAINT_HLSLI_INCLUDED

#include "config.hlsli"
#include "draw.hlsli"
#include "sdf_clip.hlsli"
#include "particle.hlsli"

static const uint BRUSH_SOLID = 1u;
static const uint BRUSH_PATTERN_RESOURCE = 7u;
static const uint BRUSH_COLOR_OFFSET = 16u;
static const uint BRUSH_IMAGE_ALPHA_OFFSET = 28u;
static const uint SDF_CANDLESTICK = 5u;
static const uint SDF_CHECKERBOARD = 14u;

uint brush_base(ConstantBuffer<CoarseConfig> settings, CoarseDraw draw) { return (settings.paint_brush_base + draw.brush_offset) * 4u; }
bool nontransparent_solid(ByteAddressBuffer paint, ConstantBuffer<CoarseConfig> settings, CoarseDraw draw) {
    if (draw.brush_offset == INVALID_INDEX) return false;
    uint base = brush_base(settings, draw);
    return paint.Load(base) == BRUSH_SOLID && paint.Load(base + BRUSH_COLOR_OFFSET) != 0u;
}
uint solid_color(ByteAddressBuffer paint, ConstantBuffer<CoarseConfig> settings, CoarseDraw draw) {
    return paint.Load(brush_base(settings, draw) + BRUSH_COLOR_OFFSET);
}
bool solid_supported_sdf(ByteAddressBuffer paint, ConstantBuffer<CoarseConfig> settings, CoarseDraw draw) {
    if (!nontransparent_solid(paint, settings, draw) || draw.sdf_offset == INVALID_INDEX || draw.shadow_offset != INVALID_INDEX || draw.sdf_len == 0u) return false;
    uint kind = paint.Load(draw.sdf_offset * 4u);
    return kind == SDF_RECT || kind == SDF_CANDLESTICK || kind == SDF_CHECKERBOARD;
}
uint full_tile_solid(ByteAddressBuffer paint, ConstantBuffer<CoarseConfig> settings, CoarseDraw draw, uint2 tile) {
    return nontransparent_solid(paint, settings, draw) && sdf_clip_covers(paint, draw, tile) ? solid_color(paint, settings, draw) : 0u;
}
bool full_tile_image(ByteAddressBuffer paint, ConstantBuffer<CoarseConfig> settings, CoarseDraw draw, uint2 tile) {
    if (draw.brush_offset == INVALID_INDEX) return false;
    uint base = brush_base(settings, draw);
    return paint.Load(base) == BRUSH_PATTERN_RESOURCE && paint.Load(base + BRUSH_IMAGE_ALPHA_OFFSET) == 255u && sdf_clip_covers(paint, draw, tile);
}
uint particle_class_flags(ByteAddressBuffer paint, ConstantBuffer<CoarseConfig> settings, CoarseDraw draw, uint tag) {
    if (tag == PTCL_COLOR) return CHUNK_CLASS_COLOR;
    if (tag == PTCL_IMAGE || (tag == PTCL_SDF && solid_supported_sdf(paint, settings, draw))) return CHUNK_CLASS_SDF;
    return CHUNK_CLASS_OTHER;
}

#endif // TILEINK_HLSL_COARSE_PAINT_HLSLI_INCLUDED
