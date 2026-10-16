SdfSample distance_to_segment(float2 p, float2 a, float2 b, float2 fallback) {
    float2 delta = b - a, nearest = a;
    float square = delta.x * delta.x + delta.y * delta.y;
    if (square >= 1.175494351e-38f && isfinite(square)) {
        float t = clamp(((p.x - a.x) * delta.x + (p.y - a.y) * delta.y) / square, 0.0f, 1.0f);
        nearest = a + delta * t;
    } else {
        float axis = max(abs(delta.x), abs(delta.y));
        if (axis > 0.0f) {
            float2 scaled = delta / axis;
            float2 projection = scaled / dot(scaled, scaled);
            float t = clamp(dot(p - a, projection) / axis, 0.0f, 1.0f);
            nearest = a + delta * t;
        }
    }
    delta = p - nearest;
    return {sdf_length(delta), normalized_or(delta, fallback)};
}
float cross_2d(float2 a, float2 b) { return a.x * b.y - a.y * b.x; }
SdfSample triangle_sample(float2 p, float2 a, float2 b, float2 c, float radius) {
    float2 ab = b - a, bc = c - b, ca = a - c;
    float2 test_b = b - a, test_c = c - a, test_p = p - a;
    float area = cross_2d(test_b, test_c);
    if (area == 0.0f || !isfinite(area)) {
        float2 span = max(max(a, b), c) - min(min(a, b), c);
        if (span.x > 0.0f && span.y > 0.0f) {
            test_b /= span; test_c /= span; test_p /= span;
            area = cross_2d(test_b, test_c);
        }
    }
    float orientation = area >= 0 ? 1 : -1;
    SdfSample nearest = nearer(nearer(
        distance_to_segment(p, a, b, normalized_or(float2(ab.y, -ab.x) * orientation, float2(1, 0))),
        distance_to_segment(p, b, c, normalized_or(float2(bc.y, -bc.x) * orientation, float2(1, 0)))),
        distance_to_segment(p, c, a, normalized_or(float2(ca.y, -ca.x) * orientation, float2(1, 0))));
    if (area != 0.0f && all(p >= min(min(a, b), c)) && all(p <= max(max(a, b), c))
        && cross_2d(test_b, test_p) * orientation >= 0
        && cross_2d(test_c - test_b, test_p - test_b) * orientation >= 0
        && cross_2d(-test_c, test_p - test_c) * orientation >= 0)
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
    // The radius is already bounded by both positive source dimensions; keep their exact scale.
    float w = half_width - radius, h = extent - radius;
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
    SdfSample nearest = empty_sdf_sample();
    bool inside = false;
    float2 previous = star_vertex(9, outer, inner);
    for (uint i = 0; i < 10; ++i) {
        float2 current = star_vertex(i, outer, inner), edge = current - previous;
        nearest = nearer(nearest, distance_to_segment(position, previous, current, normalized_or(float2(edge.y, -edge.x), float2(1, 0))));
        if ((previous.y > position.y) != (current.y > position.y)) {
            // The straddling test bounds this ratio to [0,1]. Divide before multiplying so
            // tiny/large polygons keep the same inside test without underflow/overflow.
            float fraction = (position.y - previous.y) / (current.y - previous.y);
            // Explicit fma retains the bounded ratio as its operand under global fast math.
            float crossing = fma(fraction, current.x - previous.x, previous.x);
            if (position.x < crossing) inside = !inside;
        }
        previous = current;
    }
    if (inside) nearest = {-nearest.distance, -nearest.normal};
    return {nearest.distance - max(radius, 0.0f), float2(cosine * nearest.normal.x - sine * nearest.normal.y, sine * nearest.normal.x + cosine * nearest.normal.y)};
}
