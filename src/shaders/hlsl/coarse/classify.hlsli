#ifndef TILEINK_HLSL_COARSE_CLASSIFY_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_CLASSIFY_HLSLI_INCLUDED

#include "config.hlsli"
#include "draw.hlsli"
#include "text.hlsli"
#include "sdf_clip.hlsli"

uint stack_wrapper_count(ConstantBuffer<CoarseConfig> settings, ByteAddressBuffer layers, ByteAddressBuffer draws,
    ByteAddressBuffer paths, ByteAddressBuffer backdrops, ByteAddressBuffer ranges, ByteAddressBuffer sdf, uint2 tile) {
    uint count = 0u;
    uint2 dimensions = uint2(settings.tiles_width, settings.tiles_height);
    for (uint index = settings.layer_stack_start; index < settings.layer_stack_end; index++) {
        uint2 layer = layers.Load2(index * LAYER_RECORD_STRIDE);
        if (layer.x > LAYER_BLEND) return INVALID_INDEX;
        CoarseDraw draw = load_draw(draws, layer.y);
        if (draw_has_sdf(draw)) {
            if (layer.x == LAYER_CLIP && sdf_clip_covers(sdf, draw, tile)) continue;
            if (!draw_hits_tile(draw, tile, dimensions)) return INVALID_INDEX;
        } else {
            uint backdrop = draw_backdrop(paths, draw, tile, dimensions);
            if (backdrop == INVALID_INDEX) return INVALID_INDEX;
            uint2 range = ranges.Load2(backdrop * TILE_SEGMENT_RANGE_STRIDE);
            int winding = asint(backdrops.Load(backdrop * 4u));
            bool full = draw.fill_rule == FILL_EVEN_ODD ? (asuint(winding) & 1u) != 0u : winding != 0;
            if (layer.x == LAYER_CLIP && range.x == range.y && full) continue;
            if (range.x == range.y && winding == 0) return INVALID_INDEX;
        }
        count++;
    }
    return count;
}
uint2 draw_particle_count(ConstantBuffer<CoarseConfig> settings, ByteAddressBuffer draws, ByteAddressBuffer text,
    ByteAddressBuffer paths, ByteAddressBuffer backdrops, ByteAddressBuffer ranges, ByteAddressBuffer batches, uint draw_index, uint2 tile) {
    uint batch_bytes; batches.GetDimensions(batch_bytes);
    if (draw_index >= batch_bytes / 4u || batches.Load(draw_index * 4u) != settings.draw_start) return uint2(0u,0u);
    CoarseDraw draw = load_draw(draws, draw_index);
    if (settings.text_enabled != 0u && draw.glyph_run_id != INVALID_INDEX) {
        if (draw.tag != DRAW_BRUSH) return uint2(0u,0u);
        uint glyphs = count_draw_glyphs(text, settings, draw, tile);
        return uint2(glyphs > 0u ? 1u : 0u, glyphs);
    }
    if (draw_has_sdf(draw)) return uint2(draw.tag == DRAW_BRUSH ? 1u : 0u,0u);
    uint backdrop = draw_backdrop(paths, draw, tile, uint2(settings.tiles_width, settings.tiles_height));
    if (backdrop == INVALID_INDEX || (draw.tag != DRAW_BRUSH && draw.tag != DRAW_PATH_GLYPH && draw.tag != DRAW_CLIP)) return uint2(0u,0u);
    uint2 range = ranges.Load2(backdrop * TILE_SEGMENT_RANGE_STRIDE);
    return uint2(range.x != range.y || backdrops.Load(backdrop * 4u) != 0u ? 1u : 0u,0u);
}

#endif // TILEINK_HLSL_COARSE_CLASSIFY_HLSLI_INCLUDED
