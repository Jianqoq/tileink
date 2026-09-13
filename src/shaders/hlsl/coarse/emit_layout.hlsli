// Byte offsets through the packed coarse work buffer; preceding regions remain intact.
uint tile_draw_base(uint tile) {
    return config.tile_count * COARSE_TILE_RECORD_STRIDE + config.ptcl_capacity * COARSE_PTCL_RECORD_STRIDE +
        config.glyph_capacity * 4u + tile * COARSE_TILE_DRAW_RECORD_STRIDE;
}
uint tile_emit_base(uint tile) {
    return tile_draw_base(config.tile_count) + config.tile_draw_index_count * 4u + tile * COARSE_TILE_EMIT_RECORD_STRIDE;
}
uint emit_base(uint reference) {
    return tile_emit_base(config.tile_count) + reference * COARSE_EMIT_RECORD_STRIDE;
}
