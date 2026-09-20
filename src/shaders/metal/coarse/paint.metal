bool solid(Words paint, constant CoarseConfig& c, DrawData d) {
    if (d.brush == invalid_index) return false;
    uint base = c.paint_brush_base + d.brush;
    return paint[base] == 1 && paint[base + 4] != 0;
}
uint solid_color(Words paint, constant CoarseConfig& c, DrawData d) {
    return paint[c.paint_brush_base + d.brush + 4];
}
bool full_image(Words paint, constant CoarseConfig& c, DrawData d, uint2 tile) {
    if (d.brush == invalid_index) return false;
    uint base = c.paint_brush_base + d.brush;
    return paint[base] == 7 && paint[base + 7] == 255 && sdf_clip_covers(paint, d, tile);
}
uint particle_flags(Words paint, constant CoarseConfig& c, DrawData d, uint tag) {
    if (tag == 2) return 1;
    if (tag == 13) return 2;
    if (tag == 9 && solid(paint, c, d) && d.sdf != invalid_index && d.shadow == invalid_index && d.sdf_len) {
        uint kind = paint[d.sdf];
        if (kind == 1 || kind == 5 || kind == 14) return 2;
    }
    return 4;
}
#include "tile_classification.metal"
