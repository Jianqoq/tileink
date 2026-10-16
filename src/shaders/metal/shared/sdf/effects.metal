SdfSample checkerboard_sample(float2 p, float4 rect, float cell_size) {
    float2 lower = min(rect.xy, rect.zw), upper = max(rect.xy, rect.zw);
    SdfSample outer = rect_sample(p, float4(lower, upper), float4(0));
    cell_size = max(cell_size, 0.000001f);
    float2 cell = floor((p - lower) / cell_size);
    float2 local(euclidean_remainder(p.x - lower.x, cell_size), euclidean_remainder(p.y - lower.y, cell_size));
    float edge = local.x;
    float2 normal(1, 0);
    if (cell_size - local.x < edge) { edge = cell_size - local.x; normal = float2(-1, 0); }
    if (local.y < edge) { edge = local.y; normal = float2(0, 1); }
    if (cell_size - local.y < edge) { edge = cell_size - local.y; normal = float2(0, -1); }
    bool filled = (int(cell.x) + int(cell.y)) % 2 == 0;
    return farther(outer, SdfSample{filled ? -edge : edge, normal});
}
float candlestick_coverage(float2 p, float center, float high, float low, float top, float bottom, float body_width, float wick_width, Affine inverse) {
    float half_wick = max(wick_width, 1.0f) * 0.5f;
    float wick = sdf_coverage(rect_sample(p, float4(center-half_wick, min(high,low), center+half_wick, max(high,low)), float4(0)), inverse);
    float half_body = max(body_width, 1.0f) * 0.5f, y0 = min(top,bottom), y1 = max(top,bottom);
    if (y0 == y1) { y0 -= 0.5f; y1 += 0.5f; }
    float body = sdf_coverage(rect_sample(p, float4(center-half_body, y0, center+half_body, y1), float4(0)), inverse);
    return max(wick, body);
}
