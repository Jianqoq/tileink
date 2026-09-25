bool glyph_hits_tile(Words text, constant CoarseConfig& c, DrawData d, uint index, uint2 tile) {
    uint glyph_base = c.text_run_count * 2, image_base = glyph_base + c.text_glyph_count * 3;
    Words glyph = text.offset(glyph_base + index * 3);
    if (glyph[0] == invalid_index) return false;
    Words image = text.offset(image_base + glyph[0] * 6);
    if (!image[2] || !image[3]) return false;
    int2 lower(as_type<int>(glyph[1]) + as_type<int>(image[0]), as_type<int>(glyph[2]) - as_type<int>(image[1]));
    return transformed_rect_hits_tile(d, int4(lower, lower + int2(image[2], image[3])), tile);
}
uint count_glyphs(Words text, constant CoarseConfig& c, DrawData d, uint2 tile) {
    uint start = text[d.glyph_run * 2], count = text[d.glyph_run * 2 + 1], result = 0;
    for (uint i = start; i < start + count; ++i) result += uint(glyph_hits_tile(text, c, d, i, tile));
    return result;
}
void store_glyphs(device uint* work, Words text, constant CoarseConfig& c, DrawData d, uint2 tile, uint destination) {
    uint start = text[d.glyph_run * 2], count = text[d.glyph_run * 2 + 1];
    uint base = c.tile_count * 6 + c.ptcl_capacity * 6;
    for (uint i = start; i < start + count; ++i) {
        if (glyph_hits_tile(text, c, d, i, tile)) {
            if (destination < c.glyph_capacity) work[base + destination] = i;
            ++destination;
        }
    }
}
