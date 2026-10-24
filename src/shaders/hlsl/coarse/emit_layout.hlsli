#ifndef TILEINK_HLSL_COARSE_EMIT_LAYOUT_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_EMIT_LAYOUT_HLSLI_INCLUDED

#include "config.hlsli"
#include "../coarse_records.hlsli"
// Byte offsets through the packed coarse work buffer; preceding regions remain intact.
uint tile_draw_base(ConstantBuffer<CoarseConfig> layout, uint tile) {
    return layout.tile_count * COARSE_TILE_RECORD_STRIDE + layout.ptcl_capacity * COARSE_PTCL_RECORD_STRIDE +
        layout.glyph_capacity * 4u + tile * COARSE_TILE_DRAW_RECORD_STRIDE;
}
uint tile_emit_base(ConstantBuffer<CoarseConfig> layout, uint tile) {
    return tile_draw_base(layout, layout.tile_count) + layout.tile_draw_index_count * 4u + tile * COARSE_TILE_EMIT_RECORD_STRIDE;
}
uint emit_base(ConstantBuffer<CoarseConfig> layout, uint reference) {
    return tile_emit_base(layout, layout.tile_count) + reference * COARSE_EMIT_RECORD_STRIDE;
}

#endif // TILEINK_HLSL_COARSE_EMIT_LAYOUT_HLSLI_INCLUDED
