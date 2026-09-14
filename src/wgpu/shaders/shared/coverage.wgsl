fn fill_alpha_at(
    backdrop: i32,
    fill_rule: u32,
    segment_start: u32,
    segment_end: u32,
    x: u32,
    y: u32,
) -> u32 {
    // Mirrors the CPU row-sweep accumulation order so edge pixels quantize identically.
    var base = f32(backdrop);
    var running = 0.0;
    var partial = 0.0;
    var segment_ix = segment_start;
    loop {
        if (segment_ix >= segment_end) {
            break;
        }
        let segment = segments[segment_ix];
        let parts = segment_row_parts(segment.p0x, segment.p0y, segment.p1x, segment.p1y, segment.y_edge, y);
        let y_edge = parts.x;
        let dy = parts.y;
        let xmin = parts.z;
        let xmax = parts.w;
        base += y_edge;
        if (dy != 0.0) {
            let full_start = clamp(i32(ceil(xmax)), 0, 16);
            if (full_start < 16 && i32(x) >= full_start) {
                running += dy;
            }
            let partial_start = clamp(i32(floor(xmin)), 0, 16);
            let partial_end = clamp(i32(ceil(xmax)), 0, 16);
            if (i32(x) >= partial_start && i32(x) < partial_end) {
                partial += segment_area_at(xmin, xmax, x) * dy;
            }
        }
        segment_ix += 1u;
    }
    let coverage = base + running + partial;
    return coverage_to_alpha(coverage, fill_rule);
}

fn segment_row_parts(p0x: f32, p0y: f32, p1x: f32, p1y: f32, y_edge: f32, y: u32) -> vec4<f32> {
    let delta_x = p1x - p0x;
    let delta_y = p1y - p0y;
    let row_y = f32(y);
    let local_y = p0y - row_y;
    let y0 = clamp(local_y, 0.0, 1.0);
    // Direct endpoint evaluation avoids cancellation at half-alpha boundaries.
    let y1 = clamp(p1y - row_y, 0.0, 1.0);
    let dy = y0 - y1;
    let x_sign = signum_f32(delta_x);
    let row_edge = x_sign * clamp(row_y - y_edge + 1.0, 0.0, 1.0);

    if (dy == 0.0) {
        return vec4<f32>(row_edge, dy, 0.0, 0.0);
    }

    // Use the nearer endpoint to avoid cancellation in a short row intersection.
    // An explicit slope/FMA removes the reciprocal -> fraction -> x double rounding;
    // anchoring at a distant endpoint can still push coverage over a half-channel.
    let slope = delta_x / delta_y;
    let nearer_p0 = abs(local_y - 0.5) <= abs(p1y - row_y - 0.5);
    let anchor_x = select(p1x, p0x, nearer_p0);
    let anchor_y = select(p1y - row_y, local_y, nearer_p0);
    let sx0 = fma(y0 - anchor_y, slope, anchor_x);
    let sx1 = fma(y1 - anchor_y, slope, anchor_x);
    return vec4<f32>(row_edge, dy, min(sx0, sx1), max(sx0, sx1));
}

fn segment_area_at(xmin_abs: f32, xmax_abs: f32, x: u32) -> f32 {
    let xmin = xmin_abs - f32(x);
    let xmax = xmax_abs - f32(x);
    let width = xmax - xmin;
    if (width == 0.0) {
        return clamp(1.0 - xmin, 0.0, 1.0);
    }
    // Integrate the clipped trapezoid directly. Subtracting squared endpoints
    // and dividing by a nearly vertical edge's width amplifies roundoff; shifting
    // the endpoint by epsilon also changes the covered area.
    if (xmin >= 0.0 && xmax <= 1.0) {
        return fma(-0.5, xmin + xmax, 1.0);
    }
    let left = clamp(xmin, 0.0, 1.0);
    let right = clamp(xmax, 0.0, 1.0);
    let full = clamp(-xmin, 0.0, width);
    return fma(right - left, fma(-0.5, left + right, 1.0), full) / width;
}
