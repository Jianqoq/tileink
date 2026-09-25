#include "buffer.metal"
// Raw records follow the common Rust scene ABI (31 words per draw, 19 per path).
constant uint invalid_index = 0xffffffffu;
struct DrawData {
    uint path, glyph_run, sdf, sdf_len, shadow, brush, solid_rect, tag, fill_rule;
    int4 bounds;
    float4 linear;
    float2 translation;
};
DrawData load_draw(Words records, uint index) {
    Words p = records.offset(index * 31);
    return {p[0], p[1], p[2], p[3], p[4], p[6], p[18], p[8], p[9],
        as_type<int4>(uint4(p[10], p[11], p[12], p[13])),
        as_type<float4>(uint4(p[19], p[20], p[21], p[22])), as_type<float2>(uint2(p[23], p[24]))};
}
bool has_sdf(DrawData d) { return d.sdf != invalid_index || d.shadow != invalid_index; }
bool hits_tile(DrawData d, uint2 tile, uint2 dimensions) {
    uint2 lower = min(uint2(max(d.bounds.xy, int2(0))) / 16, dimensions);
    uint2 upper = min((uint2(max(d.bounds.zw, int2(0))) + 15) / 16, dimensions);
    return all(tile >= lower) && all(tile < upper);
}
uint draw_backdrop(Words paths, DrawData d, uint2 tile, uint2 dimensions) {
    if (d.path >= paths.length / 19 || d.tag > 5 || !hits_tile(d, tile, dimensions)) return invalid_index;
    Words p = paths.offset(d.path * 19);
    uint4 box(p[6], p[7], p[8], p[9]);
    if (box.z <= box.x || any(tile < box.xy) || any(tile >= box.zw)) return invalid_index;
    return p[4] + (tile.y - box.y) * (box.z - box.x) + tile.x - box.x;
}
float2 draw_point(DrawData d, float2 p) {
    return float2(d.linear.x * p.x + d.linear.z * p.y + d.translation.x,
        d.linear.y * p.x + d.linear.w * p.y + d.translation.y);
}
bool transformed_rect_hits_tile(DrawData d, int4 rect, uint2 tile) {
    float2 a = draw_point(d, float2(rect.xy)), b = draw_point(d, float2(rect.zy));
    float2 c = draw_point(d, float2(rect.xw)), e = draw_point(d, float2(rect.zw));
    float2 lower = min(min(a, b), min(c, e)), upper = max(max(a, b), max(c, e));
    return all(lower < float2(tile * 16) + 16.0f) && all(upper > float2(tile * 16));
}
