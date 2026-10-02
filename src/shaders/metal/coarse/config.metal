// Common host CoarseConfig ABI; all coarse buffer offsets below are uint words.
struct CoarseConfig {
    uint tile_count, tiles_width, tiles_height, draw_start, draw_end;
    uint layer_stack_start, layer_stack_end, ptcl_capacity, glyph_capacity;
    uint chunk_count, text_run_count, text_glyph_count, tile_draw_index_count;
    uint emit_chunk_capacity, paint_brush_base, text_enabled;
    uint active_tile_count, active_tile_list_base, incremental;
};
static_assert(sizeof(CoarseConfig) == 76, "CoarseConfig ABI");
uint item_count(constant CoarseConfig& c) { return c.incremental ? c.active_tile_count : c.tile_count; }
uint tile_at(const device uint* work, constant CoarseConfig& c, uint item) {
    return c.incremental ? work[c.active_tile_list_base + item] : item;
}
uint tile_draw_base(constant CoarseConfig& c, uint tile) {
    return c.tile_count * 6 + c.ptcl_capacity * 6 + c.glyph_capacity + tile * 2;
}
uint tile_emit_base(constant CoarseConfig& c, uint tile) {
    return tile_draw_base(c, c.tile_count) + c.tile_draw_index_count + tile * 2;
}
uint emit_base(constant CoarseConfig& c, uint reference) {
    return tile_emit_base(c, c.tile_count) + reference * 7;
}
