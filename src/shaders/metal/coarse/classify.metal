#include "../shared/draw.metal"
#include "text.metal"
#include "sdf_clip.metal"
uint wrapper_count(constant CoarseConfig& c, Words layers, Words draws,
    Words paths, Words backdrops, const device packed_uint2* ranges,
    Words sdf, uint2 tile) {
    uint count = 0;
    for (uint i = c.layer_stack_start; i < c.layer_stack_end; ++i) {
        uint kind = layers[i * 3];
        if (kind > 2) return invalid_index;
        DrawData d = load_draw(draws, layers[i * 3 + 1]);
        if (has_sdf(d)) {
            if (!kind && sdf_clip_covers(sdf, d, tile)) continue;
            if (!hits_tile(d, tile, uint2(c.tiles_width, c.tiles_height))) return invalid_index;
        } else {
            uint backdrop = draw_backdrop(paths, d, tile, uint2(c.tiles_width, c.tiles_height));
            if (backdrop == invalid_index) return invalid_index;
            uint2 range(ranges[backdrop]);
            uint winding = backdrops[backdrop];
            bool full = d.fill_rule == 1 ? (winding & 1) != 0 : winding != 0;
            if (!kind && range.x == range.y && full) continue;
            if (range.x == range.y && !winding) return invalid_index;
        }
        ++count;
    }
    return count;
}
uint2 particle_count(constant CoarseConfig& c, Words draws, Words text,
    Words paths, Words backdrops, const device packed_uint2* ranges,
    Words batches, uint index, uint2 tile) {
    if (index >= batches.length || batches[index] != c.draw_start) return uint2(0);
    DrawData d = load_draw(draws, index);
    if (c.text_enabled && d.glyph_run != invalid_index) {
        if (d.tag) return uint2(0);
        uint glyphs = count_glyphs(text, c, d, tile);
        return uint2(glyphs > 0, glyphs);
    }
    if (has_sdf(d)) return uint2(d.tag == 0, 0);
    uint backdrop = draw_backdrop(paths, d, tile, uint2(c.tiles_width, c.tiles_height));
    if (backdrop == invalid_index || (d.tag != 0 && d.tag != 1 && d.tag != 5)) return uint2(0);
    uint2 range(ranges[backdrop]);
    return uint2(range.x != range.y || backdrops[backdrop] != 0, 0);
}
