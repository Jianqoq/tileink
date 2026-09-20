#include "particle.metal"
#include "paint.metal"
Particle draw_particle(constant CoarseConfig& c, Words draws, Words text,
    Words paint, Words paths, Words backdrops,
    const device packed_uint2* ranges, Words batches, uint index, uint2 tile) {
    Particle p = empty_particle();
    if (index >= batches.length || batches[index] != c.draw_start) return p;
    DrawData d = load_draw(draws, index);
    if (c.text_enabled && d.glyph_run != invalid_index) {
        if (!d.tag) { p.glyph_count = count_glyphs(text, c, d, tile); p.valid = p.glyph_count > 0; p.tag = 10; p.color = index; }
        return p;
    }
    if (has_sdf(d)) {
        if (!d.tag) {
            p.valid = true;
            uint color = solid(paint, c, d) && sdf_clip_covers(paint, d, tile) ? solid_color(paint, c, d) : 0;
            if (color) { p.tag = 2; p.color = color; }
            else if (full_image(paint, c, d, tile)) { p.tag = 13; p.color = index; }
            else { p.tag = 9; p.segments.x = index; p.color = index; }
        }
        return p;
    }
    uint backdrop = draw_backdrop(paths, d, tile, uint2(c.tiles_width, c.tiles_height));
    if (backdrop == invalid_index || (d.tag != 0 && d.tag != 1 && d.tag != 5)) return p;
    uint2 range(ranges[backdrop]);
    uint winding = backdrops[backdrop];
    if (range.x == range.y && !winding) return p;
    p.valid = true; p.winding = winding; p.fill_rule = d.fill_rule; p.segments = range;
    if (d.tag == 1) p.tag = 3;
    else if (d.tag == 5) p.tag = 11;
    else if (d.solid_rect && d.linear.y == 0 && d.linear.z == 0 && solid(paint, c, d) && range.x == range.y) p.tag = 2;
    if (p.tag == 2) p.color = solid_color(paint, c, d);
    else if (d.tag == 0 || d.tag == 5) p.color = index;
    return p;
}
