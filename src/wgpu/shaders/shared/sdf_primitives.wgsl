fn normalized_or(value: vec2<f32>, fallback: vec2<f32>) -> vec2<f32> {
    let magnitude = length(value);
    if (magnitude > 0.000001) {
        return value / magnitude;
    }
    return fallback;
}

fn nearer_sdf_sample(a: SdfSample, b: SdfSample) -> SdfSample {
    if (a.distance <= b.distance) {
        return a;
    }
    return b;
}

fn farther_sdf_sample(a: SdfSample, b: SdfSample) -> SdfSample {
    if (a.distance >= b.distance) {
        return a;
    }
    return b;
}

fn checkerboard_sdf_sample(
    x: f32,
    y: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    cell_size_raw: f32,
) -> SdfSample {
    let left = min(x0, x1);
    let top = min(y0, y1);
    let right = max(x0, x1);
    let bottom = max(y0, y1);
    let outer = rect_sdf_sample(x, y, left, top, right, bottom, 0.0, 0.0, 0.0, 0.0);
    let cell_size = max(cell_size_raw, 0.000001);
    let cell_x = floor((x - left) / cell_size);
    let cell_y = floor((y - top) / cell_size);
    let local_x = rem_euclid_f32(x - left, cell_size);
    let local_y = rem_euclid_f32(y - top, cell_size);
    var edge_distance = local_x;
    var normal = vec2<f32>(1.0, 0.0);
    if (cell_size - local_x < edge_distance) {
        edge_distance = cell_size - local_x;
        normal = vec2<f32>(-1.0, 0.0);
    }
    if (local_y < edge_distance) {
        edge_distance = local_y;
        normal = vec2<f32>(0.0, 1.0);
    }
    if (cell_size - local_y < edge_distance) {
        edge_distance = cell_size - local_y;
        normal = vec2<f32>(0.0, -1.0);
    }
    let filled = (i32(cell_x) + i32(cell_y)) % 2 == 0;
    let cell = SdfSample(select(edge_distance, -edge_distance, filled), normal);
    return farther_sdf_sample(outer, cell);
}

fn line_local_to_canvas_sample(sample: SdfSample, ux: f32, uy: f32) -> SdfSample {
    return SdfSample(
        sample.distance,
        vec2<f32>(
            sample.normal.x * ux - sample.normal.y * uy,
            sample.normal.x * uy + sample.normal.y * ux,
        ),
    );
}

fn dash_line_sdf_sample(
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
) -> SdfSample {
    let dash_length = max(dash_length_raw, 0.0);
    let gap_length = max(gap_length_raw, 0.0);
    var sample = line_sdf_sample(x, y, sx, sy, ex, ey, width, cap);
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
            let local_sample = nearer_sdf_sample(
                nearer_sdf_sample(
                    dash_line_segment_sample(axis, normal, len, half, cap, dash_length, cycle, offset, base - 1.0),
                    dash_line_segment_sample(axis, normal, len, half, cap, dash_length, cycle, offset, base),
                ),
                dash_line_segment_sample(axis, normal, len, half, cap, dash_length, cycle, offset, base + 1.0),
            );
            sample = line_local_to_canvas_sample(local_sample, ux, uy);
        }
    }
    return sample;
}

fn dash_line_segment_sample(
    axis: f32,
    normal: f32,
    len: f32,
    half: f32,
    cap: f32,
    dash_length: f32,
    cycle: f32,
    offset: f32,
    dash_ix: f32,
) -> SdfSample {
    let dash_start = dash_ix * cycle - offset;
    let dash_end = dash_start + dash_length;
    var sample = SdfSample(1000000.0, vec2<f32>(1.0, 0.0));
    if (dash_end > 0.0 && dash_start < len) {
        let start = max(dash_start, 0.0);
        let end = min(dash_end, len);
        if (end > start) {
            sample = line_segment_sdf_sample(axis, normal, start, end, half, cap);
        }
    }
    return sample;
}

fn line_segment_sdf_sample(axis: f32, normal: f32, start: f32, end: f32, half: f32, cap: f32) -> SdfSample {
    let nearest = clamp(axis, start, end);
    let delta = vec2<f32>(axis - nearest, normal);
    var sample = SdfSample(length(delta) - half, normalized_or(delta, vec2<f32>(0.0, 1.0)));
    if (cap < 0.5) {
        sample = local_line_rect_sample(axis, normal, start, end, half);
    } else if (cap < 1.5) {
        sample = local_line_rect_sample(axis, normal, start - half, end + half, half);
    }
    return sample;
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
    inverse_transform: AffineRecord,
) -> f32 {
    let wick_half_width = max(wick_width, 1.0) * 0.5;
    let wick = sdf_coverage_from_sample(rect_sdf_sample(
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
    ), inverse_transform);
    let half_width = max(body_width, 1.0) * 0.5;
    var body_y0 = min(body_top_y, body_bottom_y);
    var body_y1 = max(body_top_y, body_bottom_y);
    if (body_y0 == body_y1) {
        body_y0 = body_y0 - 0.5;
        body_y1 = body_y1 + 0.5;
    }
    let body = sdf_coverage_from_sample(rect_sdf_sample(
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
    ), inverse_transform);
    return max(wick, body);
}

fn line_sdf_sample(x: f32, y: f32, sx: f32, sy: f32, ex: f32, ey: f32, width: f32, cap: f32) -> SdfSample {
    let half = max(width, 0.0) * 0.5;
    let dx = ex - sx;
    let dy = ey - sy;
    let len = sqrt(dx * dx + dy * dy);
    var sample = SdfSample(1000000.0, vec2<f32>(1.0, 0.0));
    if (len <= 0.000001) {
        if (cap >= 0.5) {
            if (cap > 1.5) {
                let delta = vec2<f32>(x - sx, y - sy);
                sample = SdfSample(length(delta) - half, normalized_or(delta, vec2<f32>(1.0, 0.0)));
            } else {
                sample = local_line_rect_sample(x - sx, y - sy, -half, half, half);
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
            sample = local_line_rect_sample(axis, normal, 0.0, len, half);
        } else if (cap < 1.5) {
            sample = local_line_rect_sample(axis, normal, -half, len + half, half);
        } else {
            let nearest = clamp(axis, 0.0, len);
            let delta = vec2<f32>(axis - nearest, normal);
            sample = SdfSample(length(delta) - half, normalized_or(delta, vec2<f32>(0.0, 1.0)));
        }
        sample = line_local_to_canvas_sample(sample, ux, uy);
    }
    return sample;
}

fn arc_sdf_sample(
    x: f32,
    y: f32,
    cx: f32,
    cy: f32,
    radius_raw: f32,
    width_raw: f32,
    start_angle: f32,
    sweep_angle: f32,
    cap: f32,
) -> SdfSample {
    let radius = max(radius_raw, 0.0);
    let width = max(width_raw, 0.0);
    var sample = SdfSample(1000000.0, vec2<f32>(1.0, 0.0));
    if (radius > 0.000001 && width > 0.000001 && abs(sweep_angle) > 0.000001) {
        let vx = x - cx;
        let vy = y - cy;
        let len = sqrt(vx * vx + vy * vy);
        let half = width * 0.5;
        if (abs(sweep_angle) >= 6.2830853) {
            let radial_normal = normalized_or(vec2<f32>(vx, vy), vec2<f32>(1.0, 0.0));
            sample = SdfSample(abs(len - radius) - half, radial_normal * select(-1.0, 1.0, len >= radius));
        } else {
            sample = arc_butt_sdf_sample(vx, vy, len, radius, half, start_angle, sweep_angle);
            if (cap > 1.5) {
                sample = nearer_sdf_sample(
                    nearer_sdf_sample(sample, arc_endpoint_sdf_sample(vx, vy, radius, start_angle, half)),
                    arc_endpoint_sdf_sample(vx, vy, radius, start_angle + sweep_angle, half),
                );
            } else if (cap >= 0.5) {
                sample = nearer_sdf_sample(
                    nearer_sdf_sample(
                        sample,
                        arc_square_cap_sdf_sample(vx, vy, radius, start_angle, sweep_angle, -half, 0.0, half),
                    ),
                    arc_square_cap_sdf_sample(
                        vx,
                        vy,
                        radius,
                        start_angle + sweep_angle,
                        sweep_angle,
                        0.0,
                        half,
                        half,
                    ),
                );
            }
        }
    }
    return sample;
}

fn arc_butt_sdf_sample(
    vx: f32,
    vy: f32,
    len: f32,
    radius: f32,
    half: f32,
    start_angle: f32,
    sweep_angle: f32,
) -> SdfSample {
    if (len <= 0.000001) {
        return nearer_sdf_sample(
            arc_endpoint_sdf_sample(vx, vy, radius, start_angle, half),
            arc_endpoint_sdf_sample(vx, vy, radius, start_angle + sweep_angle, half),
        );
    }
    let angle = atan2(vy, vx);
    let radial = abs(len - radius) - half;
    if (arc_angle_in_sweep(angle, start_angle, sweep_angle)) {
        let normal = vec2<f32>(vx, vy) / len * select(-1.0, 1.0, len >= radius);
        return SdfSample(radial, normal);
    }
    return nearer_sdf_sample(
        arc_cap_segment_sdf_sample(vx, vy, radius, start_angle, half, sweep_angle, false),
        arc_cap_segment_sdf_sample(
            vx,
            vy,
            radius,
            start_angle + sweep_angle,
            half,
            sweep_angle,
            true,
        ),
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

fn arc_endpoint_sdf_sample(vx: f32, vy: f32, radius: f32, angle: f32, half: f32) -> SdfSample {
    let ex = radius * cos(angle);
    let ey = radius * sin(angle);
    let delta = vec2<f32>(vx - ex, vy - ey);
    return SdfSample(length(delta) - half, normalized_or(delta, vec2<f32>(cos(angle), sin(angle))));
}

fn arc_cap_segment_sdf_sample(
    vx: f32,
    vy: f32,
    radius: f32,
    angle: f32,
    half: f32,
    sweep_angle: f32,
    end_cap: bool,
) -> SdfSample {
    let inner_radius = max(radius - half, 0.0);
    let outer_radius = radius + half;
    let co = cos(angle);
    let si = sin(angle);
    let direction = select(-1.0, 1.0, sweep_angle >= 0.0);
    let tangent = vec2<f32>(-si * direction, co * direction);
    let fallback = select(-tangent, tangent, end_cap);
    return distance_to_segment_sdf_sample(
        vx,
        vy,
        inner_radius * co,
        inner_radius * si,
        outer_radius * co,
        outer_radius * si,
        fallback,
    );
}

fn arc_square_cap_sdf_sample(
    vx: f32,
    vy: f32,
    radius: f32,
    angle: f32,
    sweep_angle: f32,
    x0: f32,
    x1: f32,
    half: f32,
) -> SdfSample {
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
    return line_local_to_canvas_sample(
        local_line_rect_sample(local_x, local_y, x0, x1, half),
        tangent_x,
        tangent_y,
    );
}

fn distance_to_segment_sdf_sample(
    px: f32,
    py: f32,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    fallback_normal: vec2<f32>,
) -> SdfSample {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    var nearest = vec2<f32>(ax, ay);
    if (len2 > 0.000001) {
        let t = clamp(((px - ax) * dx + (py - ay) * dy) / len2, 0.0, 1.0);
        nearest = vec2<f32>(ax + dx * t, ay + dy * t);
    }
    let delta = vec2<f32>(px, py) - nearest;
    return SdfSample(length(delta), normalized_or(delta, fallback_normal));
}

fn cross_2d(a: vec2<f32>, b: vec2<f32>) -> f32 {
    return a.x * b.y - a.y * b.x;
}

fn triangle_sdf_sample(
    x: f32,
    y: f32,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    cx: f32,
    cy: f32,
    corner_radius: f32,
) -> SdfSample {
    let p = vec2<f32>(x, y);
    let a = vec2<f32>(ax, ay);
    let b = vec2<f32>(bx, by);
    let c = vec2<f32>(cx, cy);
    let ab = b - a;
    let bc = c - b;
    let ca = a - c;
    let orientation = select(-1.0, 1.0, cross_2d(ab, c - a) >= 0.0);
    var sample = nearer_sdf_sample(
        distance_to_segment_sdf_sample(
            x,
            y,
            ax,
            ay,
            bx,
            by,
            normalized_or(vec2<f32>(ab.y, -ab.x) * orientation, vec2<f32>(1.0, 0.0)),
        ),
        distance_to_segment_sdf_sample(
            x,
            y,
            bx,
            by,
            cx,
            cy,
            normalized_or(vec2<f32>(bc.y, -bc.x) * orientation, vec2<f32>(1.0, 0.0)),
        ),
    );
    sample = nearer_sdf_sample(
        sample,
        distance_to_segment_sdf_sample(
            x,
            y,
            cx,
            cy,
            ax,
            ay,
            normalized_or(vec2<f32>(ca.y, -ca.x) * orientation, vec2<f32>(1.0, 0.0)),
        ),
    );
    let side_ab = cross_2d(ab, p - a) * orientation;
    let side_bc = cross_2d(bc, p - b) * orientation;
    let side_ca = cross_2d(ca, p - c) * orientation;
    if (side_ab >= 0.0 && side_bc >= 0.0 && side_ca >= 0.0) {
        sample = SdfSample(-sample.distance, -sample.normal);
    }
    return SdfSample(sample.distance - max(corner_radius, 0.0), sample.normal);
}

fn star_vertex(index: u32, outer_radius: f32, inner_radius: f32) -> vec2<f32> {
    let radius = select(inner_radius, outer_radius, (index & 1u) == 0u);
    let angle = f32(index) * 0.6283185307179586;
    return vec2<f32>(cos(angle), sin(angle)) * radius;
}

fn star_sdf_sample(
    x: f32,
    y: f32,
    center_x: f32,
    center_y: f32,
    outer_radius: f32,
    inner_radius: f32,
    corner_radius: f32,
    rotation_radians: f32,
) -> SdfSample {
    let cosine = cos(rotation_radians);
    let sine = sin(rotation_radians);
    let relative = vec2<f32>(x - center_x, y - center_y);
    let point = vec2<f32>(
        cosine * relative.x + sine * relative.y,
        -sine * relative.x + cosine * relative.y,
    );
    var nearest = SdfSample(1.0e20, vec2<f32>(1.0, 0.0));
    var inside = false;
    var previous = star_vertex(9u, outer_radius, inner_radius);
    for (var index = 0u; index < 10u; index = index + 1u) {
        let current = star_vertex(index, outer_radius, inner_radius);
        let edge = current - previous;
        nearest = nearer_sdf_sample(
            nearest,
            distance_to_segment_sdf_sample(
                point.x,
                point.y,
                previous.x,
                previous.y,
                current.x,
                current.y,
                normalized_or(vec2<f32>(edge.y, -edge.x), vec2<f32>(1.0, 0.0)),
            ),
        );
        if ((previous.y > point.y) != (current.y > point.y)) {
            let crossing_x = previous.x
                + (point.y - previous.y) * (current.x - previous.x)
                    / (current.y - previous.y);
            if (point.x < crossing_x) {
                inside = !inside;
            }
        }
        previous = current;
    }
    if (inside) {
        nearest = SdfSample(-nearest.distance, -nearest.normal);
    }
    let local = SdfSample(nearest.distance - max(corner_radius, 0.0), nearest.normal);
    return SdfSample(
        local.distance,
        vec2<f32>(
            cosine * local.normal.x - sine * local.normal.y,
            sine * local.normal.x + cosine * local.normal.y,
        ),
    );
}

fn star_stroke_sdf_sample(
    x: f32,
    y: f32,
    center_x: f32,
    center_y: f32,
    outer_radius: f32,
    inner_radius: f32,
    corner_radius: f32,
    rotation_radians: f32,
    half_width: f32,
) -> SdfSample {
    let star = star_sdf_sample(
        x,
        y,
        center_x,
        center_y,
        outer_radius,
        inner_radius,
        corner_radius,
        rotation_radians,
    );
    return SdfSample(
        abs(star.distance) - max(half_width, 0.0),
        select(-star.normal, star.normal, star.distance >= 0.0),
    );
}

fn local_line_rect_distance(axis: f32, normal: f32, x0: f32, x1: f32, half_height: f32) -> f32 {
    return local_line_rect_sample(axis, normal, x0, x1, half_height).distance;
}

fn local_line_rect_sample(axis: f32, normal: f32, x0: f32, x1: f32, half_height: f32) -> SdfSample {
    let center = (x0 + x1) * 0.5;
    let half_width = (x1 - x0) * 0.5;
    return rounded_box_sdf_sample(axis - center, normal, half_width, half_height, 0.0);
}

fn sdf_coverage_from_dist(dist: f32) -> f32 {
    return clamp(0.5 - dist, 0.0, 1.0);
}

fn circle_sdf_distance(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> f32 {
    return circle_sdf_sample(x, y, cx, cy, radius).distance;
}

fn circle_sdf_sample(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> SdfSample {
    let dx = x - cx;
    let dy = y - cy;
    let delta = vec2<f32>(dx, dy);
    return SdfSample(length(delta) - radius, normalized_or(delta, vec2<f32>(1.0, 0.0)));
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
    return rect_sdf_sample(
        x,
        y,
        x0_raw,
        y0_raw,
        x1_raw,
        y1_raw,
        top_left,
        top_right,
        bottom_left,
        bottom_right,
    ).distance;
}

fn rect_sdf_sample(
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
) -> SdfSample {
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
    return rounded_box_sdf_sample(px, py, hx, hy, r);
}

fn rounded_box_sdf_sample(px: f32, py: f32, hx: f32, hy: f32, radius: f32) -> SdfSample {
    let q = vec2<f32>(abs(px) - hx + radius, abs(py) - hy + radius);
    let outside = max(q, vec2<f32>(0.0));
    let distance = min(max(q.x, q.y), 0.0) + length(outside) - radius;
    let signs = vec2<f32>(select(-1.0, 1.0, px >= 0.0), select(-1.0, 1.0, py >= 0.0));
    if (dot(outside, outside) > 0.000000000001) {
        return SdfSample(distance, normalize(outside) * signs);
    }
    if (q.x > q.y) {
        return SdfSample(distance, vec2<f32>(signs.x, 0.0));
    }
    return SdfSample(distance, vec2<f32>(0.0, signs.y));
}
