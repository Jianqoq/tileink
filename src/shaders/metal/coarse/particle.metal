struct Particle {
    bool valid;
    uint glyph_count, tag, winding, fill_rule;
    uint2 segments;
    uint color;
};
Particle empty_particle() { return {false, 0, 1, 0, 0, uint2(0), 0}; }
void store_particle(device uint* work, constant CoarseConfig& c, uint destination,
    uint tag, uint winding, uint fill_rule, uint2 segments, uint color) {
    if (destination >= c.ptcl_capacity) return;
    device uint* p = work + c.tile_count * 6 + destination * 6;
    p[0] = tag; p[1] = winding; p[2] = fill_rule; p[3] = segments.x; p[4] = segments.y; p[5] = color;
}
void store_particle(device uint* work, constant CoarseConfig& c, uint destination, Particle p) {
    store_particle(work, c, destination, p.tag, p.winding, p.fill_rule, p.segments, p.color);
}
