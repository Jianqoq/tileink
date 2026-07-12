const SDF_NONE_REF: u32 = 0xffffffffu;

fn sdf_coverage_from_draw(draw: DrawRecord, x: f32, y: f32) -> f32 {
    let local = affine_record_point(draw.inverse_transform, vec2<f32>(x, y));
    if (draw.sdf_offset != SDF_NONE_REF) {
        return sdf_coverage_from_blob(draw.sdf_offset, false, local.x, local.y);
    }
    if (draw.sdf_shadow_offset != SDF_NONE_REF) {
        return sdf_coverage_from_blob(draw.sdf_shadow_offset, true, local.x, local.y);
    }
    return 0.0;
}

fn affine_record_point(transform: AffineRecord, point: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        transform.a * point.x + transform.c * point.y + transform.e,
        transform.b * point.x + transform.d * point.y + transform.f,
    );
}

fn sdf_coverage_from_blob(base: u32, shadow_blob: bool, x: f32, y: f32) -> f32 {
    let kind = sdf_word(base, 0u, shadow_blob);
    let x0 = sdf_float(base, 1u, shadow_blob);
    let y0 = sdf_float(base, 2u, shadow_blob);
    let x1 = sdf_float(base, 3u, shadow_blob);
    let y1 = sdf_float(base, 4u, shadow_blob);
    let r0 = sdf_float(base, 5u, shadow_blob);
    let r1 = sdf_float(base, 6u, shadow_blob);
    let r2 = sdf_float(base, 7u, shadow_blob);
    let r3 = sdf_float(base, 8u, shadow_blob);
    let stroke_top = sdf_float(base, 9u, shadow_blob);
    let stroke_right = sdf_float(base, 10u, shadow_blob);
    let stroke_bottom = sdf_float(base, 11u, shadow_blob);
    let stroke_left = sdf_float(base, 12u, shadow_blob);
    let shadow_offset_x = sdf_float(base, 13u, shadow_blob);
    let shadow_offset_y = sdf_float(base, 14u, shadow_blob);
    let shadow_expand = sdf_float(base, 15u, shadow_blob);
    let shadow_intensity = sdf_float(base, 16u, shadow_blob);

    if (kind == GPU_SDF_RECT) {
        return sdf_coverage_from_dist(rect_sdf_distance(x, y, x0, y0, x1, y1, r0, r1, r2, r3));
    }
    if (kind == GPU_SDF_RECT_STROKE) {
        let half_top = max(stroke_top, 0.0);
        let half_right = max(stroke_right, 0.0);
        let half_bottom = max(stroke_bottom, 0.0);
        let half_left = max(stroke_left, 0.0);
        let rx0 = min(x0, x1);
        let ry0 = min(y0, y1);
        let rx1 = max(x0, x1);
        let ry1 = max(y0, y1);
        let outer = sdf_coverage_from_dist(rect_sdf_distance(
            x,
            y,
            rx0 - half_left,
            ry0 - half_top,
            rx1 + half_right,
            ry1 + half_bottom,
            r0 + max(half_top, half_left),
            r1 + max(half_top, half_right),
            r2 + max(half_bottom, half_left),
            r3 + max(half_bottom, half_right),
        ));
        let inner_x0 = rx0 + half_left;
        let inner_y0 = ry0 + half_top;
        let inner_x1 = rx1 - half_right;
        let inner_y1 = ry1 - half_bottom;
        var inner = 0.0;
        if (inner_x0 < inner_x1 && inner_y0 < inner_y1) {
            inner = sdf_coverage_from_dist(rect_sdf_distance(
                x,
                y,
                inner_x0,
                inner_y0,
                inner_x1,
                inner_y1,
                max(r0 - max(half_top, half_left), 0.0),
                max(r1 - max(half_top, half_right), 0.0),
                max(r2 - max(half_bottom, half_left), 0.0),
                max(r3 - max(half_bottom, half_right), 0.0),
            ));
        }
        return clamp(outer - inner, 0.0, 1.0);
    }
    if (kind == GPU_SDF_RECT_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            rect_sdf_distance(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
                r3,
            ),
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_CIRCLE) {
        return sdf_coverage_from_dist(circle_sdf_distance(x, y, x0, y0, x1));
    }
    if (kind == GPU_SDF_CIRCLE_STROKE) {
        let half = max(stroke_top, 0.0);
        let radius = max(x1, 0.0);
        let outer = sdf_coverage_from_dist(circle_sdf_distance(x, y, x0, y0, radius + half));
        var inner = 0.0;
        if (radius > half) {
            inner = sdf_coverage_from_dist(circle_sdf_distance(x, y, x0, y0, radius - half));
        }
        return clamp(outer - inner, 0.0, 1.0);
    }
    if (kind == GPU_SDF_CIRCLE_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            circle_sdf_distance(x - shadow_offset_x, y - shadow_offset_y, x0, y0, x1),
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_ARC) {
        return sdf_coverage_from_dist(arc_sdf_distance(x, y, x0, y0, x1, y1, r0, r1, r2));
    }
    if (kind == GPU_SDF_ARC_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            arc_sdf_distance(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
            ),
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_CANDLESTICK) {
        return candlestick_sdf_coverage(x, y, x0, y0, x1, y1, r0, r1, r2);
    }
    if (kind == GPU_SDF_LINE) {
        return sdf_coverage_from_dist(line_sdf_distance(x, y, x0, y0, x1, y1, r0, r1));
    }
    if (kind == GPU_SDF_DASH_LINE) {
        return sdf_coverage_from_dist(dash_line_sdf_distance(x, y, x0, y0, x1, y1, r0, r1, r2, r3, stroke_top));
    }
    if (kind == GPU_SDF_LINE_SHADOW) {
        return sdf_shadow_coverage_from_dist(
            line_sdf_distance(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
            ),
            shadow_expand,
            shadow_intensity,
        );
    }
    return 0.0;
}

fn sdf_word(base: u32, index: u32, shadow_blob: bool) -> u32 {
    return sdf_storage_word(base + index, shadow_blob);
}

fn sdf_float(base: u32, index: u32, shadow_blob: bool) -> f32 {
    return bitcast<f32>(sdf_word(base, index, shadow_blob));
}

fn dash_line_sdf_distance(
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    ex: f32,
    ey: f32,
    width: f32,
    cap: f32,
    dash_length_raw: f32,
    gap_length_raw: f32,
    dash_offset: f32,
) -> f32 {
    let dash_length = max(dash_length_raw, 0.0);
    let gap_length = max(gap_length_raw, 0.0);
    var dist = line_sdf_distance(x, y, sx, sy, ex, ey, width, cap);
    if (dash_length > 0.000001 && gap_length > 0.000001) {
        let half = max(width, 0.0) * 0.5;
        let dx = ex - sx;
        let dy = ey - sy;
        let len = sqrt(dx * dx + dy * dy);
        if (len > 0.000001) {
            let ux = dx / len;
            let uy = dy / len;
            let px = x - sx;
            let py = y - sy;
            let axis = px * ux + py * uy;
            let normal = -px * uy + py * ux;
            let cycle = dash_length + gap_length;
            let offset = rem_euclid_f32(dash_offset, cycle);
            let base = floor((axis + offset) / cycle);
            dist = min(
                min(
                    dash_line_segment_distance(axis, normal, len, half, cap, dash_length, cycle, offset, base - 1.0),
                    dash_line_segment_distance(axis, normal, len, half, cap, dash_length, cycle, offset, base),
                ),
                dash_line_segment_distance(axis, normal, len, half, cap, dash_length, cycle, offset, base + 1.0),
            );
        }
    }
    return dist;
}

fn dash_line_segment_distance(
    axis: f32,
    normal: f32,
    len: f32,
    half: f32,
    cap: f32,
    dash_length: f32,
    cycle: f32,
    offset: f32,
    dash_ix: f32,
) -> f32 {
    let dash_start = dash_ix * cycle - offset;
    let dash_end = dash_start + dash_length;
    var dist = 1000000.0;
    if (dash_end > 0.0 && dash_start < len) {
        let start = max(dash_start, 0.0);
        let end = min(dash_end, len);
        if (end > start) {
            dist = line_segment_sdf_distance(axis, normal, start, end, half, cap);
        }
    }
    return dist;
}

fn line_segment_sdf_distance(axis: f32, normal: f32, start: f32, end: f32, half: f32, cap: f32) -> f32 {
    let nearest = clamp(axis, start, end);
    var dist = sqrt((axis - nearest) * (axis - nearest) + normal * normal) - half;
    if (cap < 0.5) {
        dist = local_line_rect_distance(axis, normal, start, end, half);
    } else if (cap < 1.5) {
        dist = local_line_rect_distance(axis, normal, start - half, end + half, half);
    }
    return dist;
}

fn candlestick_sdf_coverage(
    x: f32,
    y: f32,
    center_x: f32,
    high_y: f32,
    low_y: f32,
    body_top_y: f32,
    body_bottom_y: f32,
    body_width: f32,
    wick_width: f32,
) -> f32 {
    let wick_half_width = max(wick_width, 1.0) * 0.5;
    let wick = sdf_coverage_from_dist(rect_sdf_distance(
        x,
        y,
        center_x - wick_half_width,
        min(high_y, low_y),
        center_x + wick_half_width,
        max(high_y, low_y),
        0.0,
        0.0,
        0.0,
        0.0,
    ));
    let half_width = max(body_width, 1.0) * 0.5;
    var body_y0 = min(body_top_y, body_bottom_y);
    var body_y1 = max(body_top_y, body_bottom_y);
    if (body_y0 == body_y1) {
        body_y0 = body_y0 - 0.5;
        body_y1 = body_y1 + 0.5;
    }
    let body = sdf_coverage_from_dist(rect_sdf_distance(
        x,
        y,
        center_x - half_width,
        body_y0,
        center_x + half_width,
        body_y1,
        0.0,
        0.0,
        0.0,
        0.0,
    ));
    return max(wick, body);
}

fn line_sdf_distance(x: f32, y: f32, sx: f32, sy: f32, ex: f32, ey: f32, width: f32, cap: f32) -> f32 {
    let half = max(width, 0.0) * 0.5;
    let dx = ex - sx;
    let dy = ey - sy;
    let len = sqrt(dx * dx + dy * dy);
    var dist = 1000000.0;
    if (len <= 0.000001) {
        if (cap >= 0.5) {
            if (cap > 1.5) {
                dist = sqrt((x - sx) * (x - sx) + (y - sy) * (y - sy)) - half;
            } else {
                dist = local_line_rect_distance(0.0, 0.0, -half, half, half);
            }
        }
    } else {
        let ux = dx / len;
        let uy = dy / len;
        let px = x - sx;
        let py = y - sy;
        let axis = px * ux + py * uy;
        let normal = -px * uy + py * ux;
        if (cap < 0.5) {
            dist = local_line_rect_distance(axis, normal, 0.0, len, half);
        } else if (cap < 1.5) {
            dist = local_line_rect_distance(axis, normal, -half, len + half, half);
        } else {
            let nearest = clamp(axis, 0.0, len);
            dist = sqrt((axis - nearest) * (axis - nearest) + normal * normal) - half;
        }
    }
    return dist;
}

fn arc_sdf_distance(
    x: f32,
    y: f32,
    cx: f32,
    cy: f32,
    radius_raw: f32,
    width_raw: f32,
    start_angle: f32,
    sweep_angle: f32,
    cap: f32,
) -> f32 {
    let radius = max(radius_raw, 0.0);
    let width = max(width_raw, 0.0);
    var dist = 1000000.0;
    if (radius > 0.000001 && width > 0.000001 && abs(sweep_angle) > 0.000001) {
        let vx = x - cx;
        let vy = y - cy;
        let len = sqrt(vx * vx + vy * vy);
        let half = width * 0.5;
        if (abs(sweep_angle) >= 6.2830853) {
            dist = abs(len - radius) - half;
        } else {
            let body = arc_butt_sdf_distance(vx, vy, len, radius, half, start_angle, sweep_angle);
            dist = body;
            if (cap > 1.5) {
                dist = min(
                    min(dist, arc_endpoint_distance(vx, vy, radius, start_angle) - half),
                    arc_endpoint_distance(vx, vy, radius, start_angle + sweep_angle) - half,
                );
            } else if (cap >= 0.5) {
                dist = min(
                    min(dist, arc_square_cap_distance(vx, vy, radius, start_angle, sweep_angle, -half, 0.0, half)),
                    arc_square_cap_distance(vx, vy, radius, start_angle + sweep_angle, sweep_angle, 0.0, half, half),
                );
            }
        }
    }
    return dist;
}

fn arc_butt_sdf_distance(vx: f32, vy: f32, len: f32, radius: f32, half: f32, start_angle: f32, sweep_angle: f32) -> f32 {
    if (len <= 0.000001) {
        return min(
            arc_endpoint_distance(vx, vy, radius, start_angle),
            arc_endpoint_distance(vx, vy, radius, start_angle + sweep_angle),
        ) - half;
    }
    let angle = atan2(vy, vx);
    let radial = abs(len - radius) - half;
    if (arc_angle_in_sweep(angle, start_angle, sweep_angle)) {
        return radial;
    }
    return min(
        arc_cap_segment_distance(vx, vy, radius, start_angle, half),
        arc_cap_segment_distance(vx, vy, radius, start_angle + sweep_angle, half),
    );
}

fn arc_angle_in_sweep(angle: f32, start_angle: f32, sweep_angle: f32) -> bool {
    let tau = 6.2831855;
    let eps = 0.000001;
    if (sweep_angle >= 0.0) {
        return rem_euclid_f32(angle - start_angle, tau) <= sweep_angle + eps;
    }
    return rem_euclid_f32(start_angle - angle, tau) <= -sweep_angle + eps;
}

fn arc_endpoint_distance(vx: f32, vy: f32, radius: f32, angle: f32) -> f32 {
    let ex = radius * cos(angle);
    let ey = radius * sin(angle);
    return sqrt((vx - ex) * (vx - ex) + (vy - ey) * (vy - ey));
}

fn arc_cap_segment_distance(vx: f32, vy: f32, radius: f32, angle: f32, half: f32) -> f32 {
    let inner_radius = max(radius - half, 0.0);
    let outer_radius = radius + half;
    let co = cos(angle);
    let si = sin(angle);
    return distance_to_segment(
        vx,
        vy,
        inner_radius * co,
        inner_radius * si,
        outer_radius * co,
        outer_radius * si,
    );
}

fn arc_square_cap_distance(
    vx: f32,
    vy: f32,
    radius: f32,
    angle: f32,
    sweep_angle: f32,
    x0: f32,
    x1: f32,
    half: f32,
) -> f32 {
    var dir = 1.0;
    if (sweep_angle < 0.0) {
        dir = -1.0;
    }
    let co = cos(angle);
    let si = sin(angle);
    let ex = radius * co;
    let ey = radius * si;
    let tangent_x = -si * dir;
    let tangent_y = co * dir;
    let px = vx - ex;
    let py = vy - ey;
    let local_x = px * tangent_x + py * tangent_y;
    let local_y = -px * tangent_y + py * tangent_x;
    return local_line_rect_distance(local_x, local_y, x0, x1, half);
}

fn distance_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    var dist = sqrt((px - ax) * (px - ax) + (py - ay) * (py - ay));
    if (len2 > 0.000001) {
        let t = clamp(((px - ax) * dx + (py - ay) * dy) / len2, 0.0, 1.0);
        let nx = ax + dx * t;
        let ny = ay + dy * t;
        dist = sqrt((px - nx) * (px - nx) + (py - ny) * (py - ny));
    }
    return dist;
}

fn local_line_rect_distance(axis: f32, normal: f32, x0: f32, x1: f32, half_height: f32) -> f32 {
    let center = (x0 + x1) * 0.5;
    let half_width = (x1 - x0) * 0.5;
    let dx = abs(axis - center) - half_width;
    let dy = abs(normal) - half_height;
    return sqrt(max(dx, 0.0) * max(dx, 0.0) + max(dy, 0.0) * max(dy, 0.0)) + min(max(dx, dy), 0.0);
}

fn sdf_coverage_from_dist(dist: f32) -> f32 {
    return clamp(0.5 - dist, 0.0, 1.0);
}

fn sdf_shadow_coverage_from_dist(dist: f32, expand: f32, intensity: f32) -> f32 {
    let clamped_intensity = clamp(intensity, 0.0, 1.0);
    var coverage = sdf_coverage_from_dist(dist) * clamped_intensity;
    if (expand > 0.0) {
        coverage = exp(-max(dist, 0.0) / expand) * clamped_intensity;
    }
    return clamp(coverage, 0.0, 1.0);
}

fn circle_sdf_distance(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> f32 {
    let dx = x - cx;
    let dy = y - cy;
    return sqrt(dx * dx + dy * dy) - radius;
}

fn rect_sdf_distance(
    x: f32,
    y: f32,
    x0_raw: f32,
    y0_raw: f32,
    x1_raw: f32,
    y1_raw: f32,
    top_left: f32,
    top_right: f32,
    bottom_left: f32,
    bottom_right: f32,
) -> f32 {
    let x0 = min(x0_raw, x1_raw);
    let y0 = min(y0_raw, y1_raw);
    let x1 = max(x0_raw, x1_raw);
    let y1 = max(y0_raw, y1_raw);
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = (x1 - x0) * 0.5;
    let hy = (y1 - y0) * 0.5;
    let px = x - cx;
    let py = y - cy;
    var radius = top_left;
    if (px >= 0.0) {
        if (py <= 0.0) {
            radius = top_right;
        } else {
            radius = bottom_right;
        }
    } else if (py > 0.0) {
        radius = bottom_left;
    }
    let r = max(min(min(radius, hx), hy), 0.0);
    let ax = abs(px);
    let ay = abs(py);

    if (r <= 0.0) {
        let dx = ax - hx;
        let dy = ay - hy;
        return sqrt(max(dx, 0.0) * max(dx, 0.0) + max(dy, 0.0) * max(dy, 0.0)) + min(max(dx, dy), 0.0);
    }

    let qx = ax - hx + r;
    let qy = ay - hy + r;
    return min(max(qx, qy), 0.0) + sqrt(max(qx, 0.0) * max(qx, 0.0) + max(qy, 0.0) * max(qy, 0.0)) - r;
}

