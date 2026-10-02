SdfSample distance_to_segment(float2 p, float2 a, float2 b, float2 fallback) {
    float2 delta = b - a, nearest = a;
    float square = delta.x * delta.x + delta.y * delta.y;
    if (square > 0.000001f) {
        float t = clamp(((p.x - a.x) * delta.x + (p.y - a.y) * delta.y) / square, 0.0f, 1.0f);
        nearest = a + delta * t;
    }
    delta = p - nearest;
    return {fast::length(delta), normalized_or(delta, fallback)};
}
float cross_2d(float2 a, float2 b) { return a.x * b.y - a.y * b.x; }
SdfSample triangle_sample(float2 p, float2 a, float2 b, float2 c, float radius) {
    float2 ab = b - a, bc = c - b, ca = a - c;
    float orientation = cross_2d(ab, c - a) >= 0 ? 1 : -1;
    SdfSample nearest = nearer(nearer(
        distance_to_segment(p, a, b, normalized_or(float2(ab.y, -ab.x) * orientation, float2(1, 0))),
        distance_to_segment(p, b, c, normalized_or(float2(bc.y, -bc.x) * orientation, float2(1, 0)))),
        distance_to_segment(p, c, a, normalized_or(float2(ca.y, -ca.x) * orientation, float2(1, 0))));
    if (cross_2d(ab, p-a) * orientation >= 0 && cross_2d(bc, p-b) * orientation >= 0 && cross_2d(ca, p-c) * orientation >= 0)
        nearest = {-nearest.distance, -nearest.normal};
    nearest.distance -= max(radius, 0.0f);
    return nearest;
}
SdfSample callout_sample(float2 p, float4 rect, float body_radius, float offset, float width, float extent, float tail_radius, float side, float visible) {
    float2 lower = min(rect.xy, rect.zw), upper = max(rect.xy, rect.zw);
    SdfSample body = rect_sample(p, float4(lower, upper), float4(body_radius));
    if (visible < 0.5f) return body;
    bool vertical = side < 0.5f || (side >= 1.5f && side < 2.5f);
    float span = vertical ? upper.x - lower.x : upper.y - lower.y;
    float half_width = min(max(width, 0.0f), span) * 0.5f;
    float center = clamp(offset, half_width, max(span - half_width, half_width));
    float radius = clamp(tail_radius, 0.0f, min(max(half_width * 0.499f, 0.0f), max(extent * 0.499f, 0.0f)));
    float w = max(half_width - radius, 0.0001f), h = max(extent - radius, 0.0001f);
    float2 a(lower.x + center - w, lower.y), b(lower.x + center + w, lower.y), c(lower.x + center, lower.y - h);
    if (side >= 0.5f && side < 1.5f) {
        a = float2(upper.x, lower.y + center - w); b = float2(upper.x, lower.y + center + w); c = float2(upper.x + h, lower.y + center);
    } else if (side >= 1.5f && side < 2.5f) {
        a = float2(lower.x + center - w, upper.y); b = float2(lower.x + center + w, upper.y); c = float2(lower.x + center, upper.y + h);
    } else if (side >= 2.5f) {
        a = float2(lower.x, lower.y + center - w); b = float2(lower.x, lower.y + center + w); c = float2(lower.x - h, lower.y + center);
    }
    return nearer(body, triangle_sample(p, a, b, c, radius));
}
SdfSample stroke_sample(SdfSample s, float half_width) {
    return {abs(s.distance) - max(half_width, 0.0f), s.distance >= 0 ? s.normal : -s.normal};
}
float2 star_vertex(uint index, float outer, float inner) {
    float radius = (index & 1) ? inner : outer, angle = float(index) * 0.6283185307179586f;
    return float2(cos(angle), sin(angle)) * radius;
}
SdfSample star_sample(float2 p, float2 center, float outer, float inner, float radius, float rotation) {
    float cosine = cos(rotation), sine = sin(rotation);
    float2 relative = p - center;
    float2 position(cosine * relative.x + sine * relative.y, -sine * relative.x + cosine * relative.y);
    SdfSample nearest{1.0e20f, float2(1, 0)};
    bool inside = false;
    float2 previous = star_vertex(9, outer, inner);
    for (uint i = 0; i < 10; ++i) {
        float2 current = star_vertex(i, outer, inner), edge = current - previous;
        nearest = nearer(nearest, distance_to_segment(position, previous, current, normalized_or(float2(edge.y, -edge.x), float2(1, 0))));
        if ((previous.y > position.y) != (current.y > position.y)) {
            float crossing = previous.x + (position.y - previous.y) * (current.x - previous.x) / (current.y - previous.y);
            if (position.x < crossing) inside = !inside;
        }
        previous = current;
    }
    if (inside) nearest = {-nearest.distance, -nearest.normal};
    return {nearest.distance - max(radius, 0.0f), float2(cosine * nearest.normal.x - sine * nearest.normal.y, sine * nearest.normal.x + cosine * nearest.normal.y)};
}
