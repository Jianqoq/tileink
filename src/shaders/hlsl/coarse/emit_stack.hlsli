#ifndef TILEINK_HLSL_COARSE_EMIT_STACK_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_EMIT_STACK_HLSLI_INCLUDED

#include "classify.hlsli"
#include "particle.hlsli"

bool noop_stack_clip(ConstantBuffer<CoarseConfig> settings, uint2 layer, CoarseDraw draw, ByteAddressBuffer paths,
    ByteAddressBuffer backdrops, ByteAddressBuffer ranges, ByteAddressBuffer paint, uint2 tile) {
    if (layer.x != LAYER_CLIP) return false;
    if (draw_has_sdf(draw)) return sdf_clip_covers(paint, draw, tile);
    uint backdrop = draw_backdrop(paths, draw, tile, uint2(settings.tiles_width, settings.tiles_height));
    if (backdrop == INVALID_INDEX) return false;
    uint2 range = ranges.Load2(backdrop * TILE_SEGMENT_RANGE_STRIDE);
    uint winding = backdrops.Load(backdrop * 4u);
    return range.x == range.y && (draw.fill_rule == FILL_EVEN_ODD ? (winding & 1u) != 0u : winding != 0u);
}
void emit_stack_begins(ConstantBuffer<CoarseConfig> settings, RWByteAddressBuffer work, ByteAddressBuffer layers,
    ByteAddressBuffer draws, ByteAddressBuffer paths, ByteAddressBuffer backdrops, ByteAddressBuffer ranges,
    ByteAddressBuffer paint, uint destination, uint2 tile) {
    for (uint index = settings.layer_stack_start; index < settings.layer_stack_end; index++) {
        uint3 layer = layers.Load3(index * LAYER_RECORD_STRIDE);
        if (layer.x > LAYER_BLEND) continue;
        CoarseDraw draw = load_draw(draws, layer.y);
        if (noop_stack_clip(settings, layer.xy, draw, paths, backdrops, ranges, paint, tile)) continue;
        if (layer.x == LAYER_CLIP && draw_has_sdf(draw)) {
            store_particle(work, settings, destination++, PTCL_BEGIN_SDF_CLIP, 0u, 0u, uint2(0u,0u), layer.y);
        } else {
            uint backdrop = draw_backdrop(paths, draw, tile, uint2(settings.tiles_width, settings.tiles_height));
            if (backdrop == INVALID_INDEX) continue;
            uint tag = layer.x == LAYER_OPACITY ? PTCL_BEGIN_OPACITY : (layer.x == LAYER_BLEND ? PTCL_BEGIN_BLEND : PTCL_BEGIN_CLIP);
            store_particle(work, settings, destination++, tag, backdrops.Load(backdrop * 4u), draw.fill_rule, ranges.Load2(backdrop * TILE_SEGMENT_RANGE_STRIDE), layer.z);
        }
    }
}
void emit_stack_ends(ConstantBuffer<CoarseConfig> settings, RWByteAddressBuffer work, ByteAddressBuffer layers,
    ByteAddressBuffer draws, ByteAddressBuffer paths, ByteAddressBuffer backdrops, ByteAddressBuffer ranges,
    ByteAddressBuffer paint, uint destination, uint2 tile) {
    for (uint index = settings.layer_stack_end; index > settings.layer_stack_start;) {
        uint3 layer = layers.Load3(--index * LAYER_RECORD_STRIDE);
        if (layer.x > LAYER_BLEND) continue;
        CoarseDraw draw = load_draw(draws, layer.y);
        if (noop_stack_clip(settings, layer.xy, draw, paths, backdrops, ranges, paint, tile)) continue;
        uint tag = layer.x == LAYER_OPACITY ? PTCL_END_OPACITY : (layer.x == LAYER_BLEND ? PTCL_END_BLEND : PTCL_END_CLIP);
        store_particle(work, settings, destination++, tag, 0u, 0u, uint2(0u,0u), 0u);
    }
}

#endif // TILEINK_HLSL_COARSE_EMIT_STACK_HLSLI_INCLUDED
