#ifndef TILEINK_HLSL_COARSE_EMIT_DRAW_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_EMIT_DRAW_HLSLI_INCLUDED

#include "config.hlsli"
#include "draw.hlsli"
#include "text.hlsli"
#include "paint.hlsli"
#include "particle.hlsli"

Particle draw_particle(ConstantBuffer<CoarseConfig> settings, ByteAddressBuffer draws, ByteAddressBuffer text,
    ByteAddressBuffer paint, ByteAddressBuffer paths, ByteAddressBuffer backdrops, ByteAddressBuffer ranges,
    ByteAddressBuffer batches, uint draw_index, uint2 tile) {
    Particle result = empty_particle();
    uint batch_bytes; batches.GetDimensions(batch_bytes);
    if (draw_index >= batch_bytes / 4u || batches.Load(draw_index * 4u) != settings.draw_start) return result;
    CoarseDraw draw = load_draw(draws, draw_index);
    if (settings.text_enabled != 0u && draw.glyph_run_id != INVALID_INDEX) {
        if (draw.tag == DRAW_BRUSH) {
            result.glyph_count = count_draw_glyphs(text, settings, draw, tile);
            result.valid = result.glyph_count > 0u;
            result.tag = PTCL_GLYPH;
            result.color = draw_index;
        }
        return result;
    }
    if (draw_has_sdf(draw)) {
        if (draw.tag == DRAW_BRUSH) {
            result.valid = true;
            uint color = full_tile_solid(paint, settings, draw, tile);
            if (color != 0u) { result.tag = PTCL_COLOR; result.color = color; }
            else if (full_tile_image(paint, settings, draw, tile)) { result.tag = PTCL_IMAGE; result.color = draw_index; }
            else { result.tag = PTCL_SDF; result.segments.x = draw_index; result.color = draw_index; }
        }
        return result;
    }
    uint backdrop = draw_backdrop(paths, draw, tile, uint2(settings.tiles_width, settings.tiles_height));
    if (backdrop == INVALID_INDEX || (draw.tag != DRAW_BRUSH && draw.tag != DRAW_PATH_GLYPH && draw.tag != DRAW_CLIP)) return result;
    uint2 range = ranges.Load2(backdrop * TILE_SEGMENT_RANGE_STRIDE);
    uint winding = backdrops.Load(backdrop * 4u);
    if (range.x == range.y && winding == 0u) return result;
    result.valid = true;
    result.winding = winding;
    result.fill_rule = draw.fill_rule;
    result.segments = range;
    if (draw.tag == DRAW_CLIP) result.tag = PTCL_BEGIN_CLIP;
    else if (draw.tag == DRAW_PATH_GLYPH) result.tag = PTCL_PATH_GLYPH;
    else if (draw.solid_rect != 0u && draw.affine_linear.y == 0.0 && draw.affine_linear.z == 0.0
        && nontransparent_solid(paint, settings, draw) && range.x == range.y) result.tag = PTCL_COLOR;
    if (result.tag == PTCL_COLOR) result.color = solid_color(paint, settings, draw);
    else if (draw.tag == DRAW_BRUSH || draw.tag == DRAW_PATH_GLYPH) result.color = draw_index;
    return result;
}

#endif // TILEINK_HLSL_COARSE_EMIT_DRAW_HLSLI_INCLUDED
