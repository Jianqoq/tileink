#ifndef TILEINK_HLSL_COARSE_TEXT_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_TEXT_HLSLI_INCLUDED

#include "../coarse_records.hlsli"
#include "config.hlsli"
#include "../shared/draw.hlsli"

bool glyph_hits_tile(ByteAddressBuffer text, ConstantBuffer<CoarseConfig> settings, DrawData draw, uint index, uint2 tile) {
    uint glyph_base = settings.text_run_count * GLYPH_RUN_STRIDE;
    uint image_base = glyph_base + settings.text_glyph_count * GLYPH_RECORD_STRIDE;
    uint3 glyph = text.Load3(glyph_base + index * GLYPH_RECORD_STRIDE);
    if (glyph.x == INVALID_INDEX) return false;
    uint4 image_record = text.Load4(image_base + glyph.x * GLYPH_IMAGE_STRIDE);
    if (image_record.z == 0u || image_record.w == 0u) return false;
    int2 lower = int2(asint(glyph.y) + asint(image_record.x), asint(glyph.z) - asint(image_record.y));
    return transformed_rect_hits_tile(draw, int4(lower, lower + int2(image_record.zw)), tile);
}
uint count_draw_glyphs(ByteAddressBuffer text, ConstantBuffer<CoarseConfig> settings, DrawData draw, uint2 tile) {
    uint2 run = text.Load2(draw.glyph_run_id * GLYPH_RUN_STRIDE);
    uint count = 0u;
    for (uint index = run.x; index < run.x + run.y; index++) {
        if (glyph_hits_tile(text, settings, draw, index, tile)) count++;
    }
    return count;
}
void store_draw_glyphs(RWByteAddressBuffer work, ByteAddressBuffer text, ConstantBuffer<CoarseConfig> settings, DrawData draw, uint2 tile, uint destination) {
    uint2 run = text.Load2(draw.glyph_run_id * GLYPH_RUN_STRIDE);
    uint base = settings.tile_count * COARSE_TILE_RECORD_STRIDE + settings.ptcl_capacity * COARSE_PTCL_RECORD_STRIDE;
    for (uint index = run.x; index < run.x + run.y; index++) {
        if (glyph_hits_tile(text, settings, draw, index, tile)) {
            if (destination < settings.glyph_capacity) work.Store(base + destination * 4u, index);
            destination++;
        }
    }
}

#endif // TILEINK_HLSL_COARSE_TEXT_HLSLI_INCLUDED
