#ifndef TILEINK_HLSL_COARSE_DRAW_LIST_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_DRAW_LIST_HLSLI_INCLUDED

#include "../constants.hlsli"
#include "../coarse_records.hlsli"
#include "config.hlsli"
#include "emit_layout.hlsli"
#include "tags.hlsli"

uint draw_index_base(ConstantBuffer<CoarseConfig> settings) { return tile_draw_base(settings, settings.tile_count); }
// Tile lists are contiguous ranges; workgroup chunks do not impose storage padding.
uint draw_page_next(RWByteAddressBuffer work, ConstantBuffer<CoarseConfig> settings, uint page) { return page + COARSE_WORKGROUP_SIZE; }
uint draw_page_index(RWByteAddressBuffer work, ConstantBuffer<CoarseConfig> settings, uint page, uint slot) {
    return work.Load(draw_index_base(settings) + (page + slot) * 4u);
}
uint draw_page_at(RWByteAddressBuffer work, ConstantBuffer<CoarseConfig> settings, uint tile, uint local_page) {
    return work.Load(tile_draw_base(settings, tile)) + local_page * COARSE_WORKGROUP_SIZE;
}
void store_tile_counts(RWByteAddressBuffer work, ConstantBuffer<CoarseConfig> settings, uint tile, uint2 count) {
    work.Store(tile * COARSE_TILE_RECORD_STRIDE, count.x);
    work.Store(tile * COARSE_TILE_RECORD_STRIDE + COARSE_TILE_GLYPH_COUNT, count.y);
    uint kind_base = emit_base(settings, settings.emit_chunk_capacity);
    work.Store(kind_base + tile * 4u, count.x > 0u ? TILE_KIND_INTERPRETER : TILE_KIND_EMPTY);
}

#endif // TILEINK_HLSL_COARSE_DRAW_LIST_HLSLI_INCLUDED
