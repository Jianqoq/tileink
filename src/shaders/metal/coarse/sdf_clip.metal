bool corner_covers(float2 point, float2 corner, float2 center, float radius, float inset) {
    bool square = radius > 0 && all(abs(point - corner) < radius);
    float inner = radius - inset;
    float2 delta = point - center;
    return !square || (inner > 0 && dot(delta, delta) <= inner * inner);
}
bool rect_covers_tile(float4 rect, float4 radii, DrawData d, uint2 tile, float inset) {
    float2 lower = min(rect.xy, rect.zw), upper = max(rect.xy, rect.zw), size = upper - lower;
    float2 a = float2(tile * 16) + 0.5f - d.translation, b = a + 15.0f;
    if (any(size < 15.0f + 2.0f * inset) || any(a < lower + inset) || any(b > upper - inset)) return false;
    // Large finite coordinates can lose the AA inset or overflow fine's center.
    // Restrict this full-tile proof; uncertain geometry keeps exact fine evaluation.
    if (!all(abs(rect) <= 65536.0f) || !all(abs(radii) <= 65536.0f)
        || !all(abs(d.translation) <= 65536.0f)) return false;
    float4 r = clamp(radii, 0.0f, min(size.x, size.y) * 0.5f);
    return corner_covers(a, lower, lower + r.x, r.x, inset)
        && corner_covers(float2(b.x, a.y), float2(upper.x, lower.y), float2(upper.x-r.y, lower.y+r.y), r.y, inset)
        && corner_covers(float2(a.x, b.y), float2(lower.x, upper.y), float2(lower.x+r.z, upper.y-r.z), r.z, inset)
        && corner_covers(b, upper, upper-r.w, r.w, inset);
}

bool sdf_clip_covers(Words sdf, DrawData d, uint2 tile) {
    if (d.sdf == invalid_index || d.shadow != invalid_index || d.sdf_len < 9) return false;
    Words p = sdf.offset(d.sdf);
    if (p[0] != 1 || any(d.linear != float4(1, 0, 0, 1))) return false;
    return rect_covers_tile(as_type<float4>(uint4(p[1], p[2], p[3], p[4])),
        as_type<float4>(uint4(p[5], p[6], p[7], p[8])), d, tile, 0.5f);
}
bool sdf_stroke_empty(Words sdf, DrawData d, uint2 tile) {
    if (d.sdf == invalid_index || d.shadow != invalid_index || d.sdf_len < 13
        || any(d.linear != float4(1, 0, 0, 1))) return false;
    Words p = sdf.offset(d.sdf);
    if (p[0] != 3) return false;
    float4 rect = as_type<float4>(uint4(p[1], p[2], p[3], p[4]));
    float4 radii = as_type<float4>(uint4(p[5], p[6], p[7], p[8]));
    float4 stroke = as_type<float4>(uint4(p[9], p[10], p[11], p[12]));
    if (!all(abs(rect) <= 65536.0f) || !all(abs(radii) <= 65536.0f)
        || !all(abs(stroke) <= 65536.0f)) return false;
    float4 half_width = max(stroke, float4(0));
    float2 lower = min(rect.xy, rect.zw), upper = max(rect.xy, rect.zw);
    float4 inner(lower.x + half_width.w, lower.y + half_width.x,
        upper.x - half_width.y, upper.y - half_width.z);
    if (inner.x >= inner.z || inner.y >= inner.w) return false;
    float4 corner(max(half_width.x, half_width.w), max(half_width.x, half_width.y),
        max(half_width.z, half_width.w), max(half_width.z, half_width.y));
    // Coverage is clamp(outer - inner, 0, 1). Remove only proven empty tiles,
    // with an additional half-pixel reserve for bounded float32 rounding.
    return rect_covers_tile(inner, max(radii - corner, float4(0)), d, tile, 1.0f);
}
