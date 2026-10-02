SdfSample rotate_line(SdfSample s, float2 axis) {
    return {s.distance, float2(s.normal.x * axis.x - s.normal.y * axis.y, s.normal.x * axis.y + s.normal.y * axis.x)};
}
SdfSample line_segment(float axis, float normal, float start, float end, float half_width, float cap) {
    if (cap < 0.5f) return local_line_rect(axis, normal, start, end, half_width);
    if (cap < 1.5f) return local_line_rect(axis, normal, start - half_width, end + half_width, half_width);
    float2 delta(axis - clamp(axis, start, end), normal);
    return {fast::length(delta) - half_width, normalized_or(delta, float2(0, 1))};
}
SdfSample line_sample(float2 p, float4 endpoints, float width, float cap) {
    float half_width = max(width, 0.0f) * 0.5f;
    float2 delta = endpoints.zw - endpoints.xy;
    float magnitude = sqrt(delta.x * delta.x + delta.y * delta.y);
    SdfSample result{1000000, float2(1, 0)};
    float2 relative = p - endpoints.xy;
    if (magnitude <= 0.000001f) {
        if (cap >= 0.5f) {
            if (cap > 1.5f) result = {fast::length(relative) - half_width, normalized_or(relative, float2(1, 0))};
            else result = local_line_rect(relative.x, relative.y, -half_width, half_width, half_width);
        }
    } else {
        float2 unit = delta / magnitude;
        float axis = relative.x * unit.x + relative.y * unit.y;
        float normal = -relative.x * unit.y + relative.y * unit.x;
        result = rotate_line(line_segment(axis, normal, 0, magnitude, half_width, cap), unit);
    }
    return result;
}
SdfSample dash_segment(float axis, float normal, float magnitude, float half_width, float cap,
    float dash, float cycle, float offset, float index) {
    float start = index * cycle - offset, end = start + dash;
    if (end > 0 && start < magnitude) {
        start = max(start, 0.0f); end = min(end, magnitude);
        if (end > start) return line_segment(axis, normal, start, end, half_width, cap);
    }
    return {1000000, float2(1, 0)};
}
SdfSample dash_line_sample(float2 p, float4 endpoints, float width, float cap, float dash, float gap, float offset) {
    SdfSample result = line_sample(p, endpoints, width, cap);
    dash = max(dash, 0.0f); gap = max(gap, 0.0f);
    if (dash <= 0.000001f || gap <= 0.000001f) return result;
    float2 delta = endpoints.zw - endpoints.xy;
    float magnitude = sqrt(delta.x * delta.x + delta.y * delta.y);
    if (magnitude <= 0.000001f) return result;
    float2 unit = delta / magnitude, relative = p - endpoints.xy;
    float axis = relative.x * unit.x + relative.y * unit.y, normal = -relative.x * unit.y + relative.y * unit.x;
    float cycle = dash + gap, half_width = max(width, 0.0f) * 0.5f;
    offset = euclidean_remainder(offset, cycle);
    float base = floor((axis + offset) / cycle);
    return rotate_line(nearer(nearer(
        dash_segment(axis, normal, magnitude, half_width, cap, dash, cycle, offset, base - 1),
        dash_segment(axis, normal, magnitude, half_width, cap, dash, cycle, offset, base)),
        dash_segment(axis, normal, magnitude, half_width, cap, dash, cycle, offset, base + 1)), unit);
}
