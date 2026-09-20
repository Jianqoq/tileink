bool noop_clip(constant CoarseConfig& c, uint kind, DrawData d, Words paths,
    Words backdrops, const device packed_uint2* ranges, Words paint, uint2 tile) {
    if (kind) return false;
    if (has_sdf(d)) return sdf_clip_covers(paint, d, tile);
    uint backdrop = draw_backdrop(paths, d, tile, uint2(c.tiles_width, c.tiles_height));
    if (backdrop == invalid_index) return false;
    uint2 range(ranges[backdrop]);
    uint winding = backdrops[backdrop];
    return range.x == range.y && (d.fill_rule == 1 ? (winding & 1) != 0 : winding != 0);
}
void emit_stack(constant CoarseConfig& c, device uint* work, Words layers,
    Words draws, Words paths, Words backdrops,
    const device packed_uint2* ranges, Words paint, uint destination, uint2 tile, bool end) {
    uint n = c.layer_stack_end - c.layer_stack_start;
    for (uint step = 0; step < n; ++step) {
        uint index = end ? c.layer_stack_end - 1 - step : c.layer_stack_start + step;
        Words layer = layers.offset(index * 3);
        uint kind = layer[0];
        if (kind > 2) continue;
        DrawData d = load_draw(draws, layer[1]);
        if (noop_clip(c, kind, d, paths, backdrops, ranges, paint, tile)) continue;
        if (end) {
            store_particle(work, c, destination++, kind == 1 ? 6 : kind == 2 ? 8 : 4, 0, 0, uint2(0), 0);
        } else if (!kind && has_sdf(d)) {
            store_particle(work, c, destination++, 12, 0, 0, uint2(0), layer[1]);
        } else {
            uint backdrop = draw_backdrop(paths, d, tile, uint2(c.tiles_width, c.tiles_height));
            if (backdrop == invalid_index) continue;
            store_particle(work, c, destination++, kind == 1 ? 5 : kind == 2 ? 7 : 3,
                backdrops[backdrop], d.fill_rule, uint2(ranges[backdrop]), layer[2]);
        }
    }
}
