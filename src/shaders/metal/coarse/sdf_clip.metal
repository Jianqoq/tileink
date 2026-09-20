bool corner_covers(float2 point, float2 corner, float2 center, float radius) {
    bool square = radius > 0 && all(abs(point - corner) < radius);
    float inner = radius - 0.5f;
    float2 delta = point - center;
    return !square || (inner > 0 && dot(delta, delta) <= inner * inner);
}
bool sdf_clip_covers(Words sdf, DrawData d, uint2 tile) {
    if (d.sdf == invalid_index || d.shadow != invalid_index || d.sdf_len < 9) return false;
    Words p = sdf.offset(d.sdf);
    if (p[0] != 1 || any(d.linear != float4(1, 0, 0, 1))) return false;
    float4 rect = as_type<float4>(uint4(p[1], p[2], p[3], p[4]));
    float2 lower = min(rect.xy, rect.zw), upper = max(rect.xy, rect.zw), size = upper - lower;
    float2 a = float2(tile * 16) + 0.5f - d.translation, b = a + 15.0f;
    if (any(size < 16.0f) || any(a < lower + 0.5f) || any(b > upper - 0.5f)) return false;
    float4 r = clamp(as_type<float4>(uint4(p[5], p[6], p[7], p[8])), 0.0f, min(size.x, size.y) * 0.5f);
    return corner_covers(a, lower, lower + r.x, r.x)
        && corner_covers(float2(b.x, a.y), float2(upper.x, lower.y), float2(upper.x-r.y, lower.y+r.y), r.y)
        && corner_covers(float2(a.x, b.y), float2(lower.x, upper.y), float2(lower.x+r.z, upper.y-r.z), r.z)
        && corner_covers(b, upper, upper-r.w, r.w);
}
