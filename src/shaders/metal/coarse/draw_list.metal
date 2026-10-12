// Tile lists are contiguous ranges; workgroup chunks do not impose storage padding.
uint page_next(const device uint* work, constant CoarseConfig& c, uint page) { return page + 256; }
uint page_index(const device uint* work, constant CoarseConfig& c, uint page, uint slot) {
    return work[tile_draw_base(c, c.tile_count) + page + slot];
}
uint page_at(const device uint* work, constant CoarseConfig& c, uint tile, uint local_page) {
    return work[tile_draw_base(c, tile)] + local_page * 256;
}
void store_counts(device uint* work, constant CoarseConfig& c, uint tile, uint2 count) {
    work[tile * 6] = count.x; work[tile * 6 + 3] = count.y;
    work[emit_base(c, c.emit_chunk_capacity) + tile] = count.x ? 0 : 1;
}
