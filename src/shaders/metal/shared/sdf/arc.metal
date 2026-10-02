bool in_sweep(float angle, float start, float sweep) {
    return sweep >= 0 ? euclidean_remainder(angle - start, 6.2831855f) <= sweep + 0.000001f
        : euclidean_remainder(start - angle, 6.2831855f) <= -sweep + 0.000001f;
}
SdfSample arc_cap(float2 p, float radius, float angle, float half_width, float sweep, bool end) {
    float inner = max(radius - half_width, 0.0f), outer = radius + half_width;
    float2 unit(cos(angle), sin(angle));
    float direction = sweep >= 0 ? 1 : -1;
    float2 tangent(-unit.y * direction, unit.x * direction);
    return distance_to_segment(p, inner * unit, outer * unit, end ? tangent : -tangent);
}
SdfSample arc_endpoint(float2 p, float radius, float angle, float half_width) {
    float2 unit(cos(angle), sin(angle)), delta = p - radius * unit;
    return {fast::length(delta) - half_width, normalized_or(delta, unit)};
}
SdfSample arc_butt(float2 p, float magnitude, float radius, float half_width, float start, float sweep) {
    if (magnitude <= 0.000001f) return nearer(arc_endpoint(p, radius, start, half_width), arc_endpoint(p, radius, start + sweep, half_width));
    if (in_sweep(atan2(p.y, p.x), start, sweep))
        return {abs(magnitude - radius) - half_width, p / magnitude * (magnitude >= radius ? 1.0f : -1.0f)};
    return nearer(arc_cap(p, radius, start, half_width, sweep, false), arc_cap(p, radius, start + sweep, half_width, sweep, true));
}
SdfSample arc_square(float2 p, float radius, float angle, float sweep, float start, float end, float half_width) {
    float direction = sweep < 0 ? -1 : 1;
    float2 unit(cos(angle), sin(angle)), tangent(-unit.y * direction, unit.x * direction);
    float2 relative = p - radius * unit;
    float axis = relative.x * tangent.x + relative.y * tangent.y, normal = -relative.x * tangent.y + relative.y * tangent.x;
    return rotate_line(local_line_rect(axis, normal, start, end, half_width), tangent);
}
SdfSample arc_sample(float2 p, float2 center, float radius, float width, float start, float sweep, float cap) {
    radius = max(radius, 0.0f); width = max(width, 0.0f);
    if (radius <= 0.000001f || width <= 0.000001f || abs(sweep) <= 0.000001f) return {1000000, float2(1, 0)};
    float2 relative = p - center;
    float magnitude = sqrt(relative.x * relative.x + relative.y * relative.y), half_width = width * 0.5f;
    if (abs(sweep) >= 6.2830853f)
        return {abs(magnitude - radius) - half_width, normalized_or(relative, float2(1, 0)) * (magnitude >= radius ? 1.0f : -1.0f)};
    SdfSample result = arc_butt(relative, magnitude, radius, half_width, start, sweep);
    if (cap > 1.5f) return nearer(nearer(result, arc_endpoint(relative, radius, start, half_width)), arc_endpoint(relative, radius, start + sweep, half_width));
    if (cap >= 0.5f) return nearer(nearer(result, arc_square(relative, radius, start, sweep, -half_width, 0, half_width)), arc_square(relative, radius, start + sweep, sweep, 0, half_width, half_width));
    return result;
}
