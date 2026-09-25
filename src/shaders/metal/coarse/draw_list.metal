uint page_next(const device uint* work, constant CoarseConfig& c, uint page) {
    return (page & 0x80000000u) ? page + 256 : work[tile_draw_base(c, c.tile_count) + page * 257];
}
uint page_index(const device uint* work, constant CoarseConfig& c, uint page, uint slot) {
    uint index = (page & 0x80000000u) ? (page & 0x7fffffffu) + slot : page * 257 + 1 + slot;
    return work[tile_draw_base(c, c.tile_count) + index];
}
uint page_at(const device uint* work, constant CoarseConfig& c, uint tile, uint local_page) {
    uint page = work[tile_draw_base(c, tile)];
    if (page & 0x80000000u) return page + local_page * 256;
    for (uint i = 0; i < local_page && page != invalid_index; ++i) page = page_next(work, c, page);
    return page;
}
void store_counts(device uint* work, constant CoarseConfig& c, uint tile, uint2 count) {
    work[tile * 6] = count.x; work[tile * 6 + 3] = count.y;
    work[emit_base(c, c.emit_chunk_capacity) + tile] = count.x ? 0 : 1;
}
