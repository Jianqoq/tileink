#[cube]
#[allow(clippy::too_many_arguments)]
fn gpu_sdf_alpha_from_encoded(
    kind: u32,
    x: f32,
    y: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    r0: f32,
    r1: f32,
    r2: f32,
    r3: f32,
    stroke_top: f32,
    stroke_right: f32,
    stroke_bottom: f32,
    stroke_left: f32,
    shadow_offset_x: f32,
    shadow_offset_y: f32,
    shadow_expand: f32,
    shadow_intensity: f32,
) -> u32 {
    let mut coverage = 0.0;
    if kind == CUBE_SDF_RECT {
        coverage = gpu_sdf_coverage_from_dist(gpu_rect_sdf_distance(
            x, y, x0, y0, x1, y1, r0, r1, r2, r3,
        ));
    } else if kind == CUBE_SDF_RECT_STROKE {
        let half_top = stroke_top.max(0.0);
        let half_right = stroke_right.max(0.0);
        let half_bottom = stroke_bottom.max(0.0);
        let half_left = stroke_left.max(0.0);
        let rx0 = x0.min(x1);
        let ry0 = y0.min(y1);
        let rx1 = x0.max(x1);
        let ry1 = y0.max(y1);
        let outer = gpu_sdf_coverage_from_dist(gpu_rect_sdf_distance(
            x,
            y,
            rx0 - half_left,
            ry0 - half_top,
            rx1 + half_right,
            ry1 + half_bottom,
            r0 + half_top.max(half_left),
            r1 + half_top.max(half_right),
            r2 + half_bottom.max(half_left),
            r3 + half_bottom.max(half_right),
        ));
        let inner_x0 = rx0 + half_left;
        let inner_y0 = ry0 + half_top;
        let inner_x1 = rx1 - half_right;
        let inner_y1 = ry1 - half_bottom;
        let mut inner = 0.0;
        if inner_x0 < inner_x1 && inner_y0 < inner_y1 {
            inner = gpu_sdf_coverage_from_dist(gpu_rect_sdf_distance(
                x,
                y,
                inner_x0,
                inner_y0,
                inner_x1,
                inner_y1,
                (r0 - half_top.max(half_left)).max(0.0),
                (r1 - half_top.max(half_right)).max(0.0),
                (r2 - half_bottom.max(half_left)).max(0.0),
                (r3 - half_bottom.max(half_right)).max(0.0),
            ));
        }
        coverage = (outer - inner).clamp(0.0, 1.0);
    } else if kind == CUBE_SDF_RECT_SHADOW {
        coverage = gpu_sdf_shadow_coverage_from_dist(
            gpu_rect_sdf_distance(
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
    } else if kind == CUBE_SDF_CIRCLE {
        coverage = gpu_sdf_coverage_from_dist(gpu_circle_sdf_distance(x, y, x0, y0, x1));
    } else if kind == CUBE_SDF_CIRCLE_STROKE {
        let half = stroke_top.max(0.0);
        let radius = x1.max(0.0);
        let outer =
            gpu_sdf_coverage_from_dist(gpu_circle_sdf_distance(x, y, x0, y0, radius + half));
        let mut inner = 0.0;
        if radius > half {
            inner =
                gpu_sdf_coverage_from_dist(gpu_circle_sdf_distance(x, y, x0, y0, radius - half));
        }
        coverage = (outer - inner).clamp(0.0, 1.0);
    } else if kind == CUBE_SDF_CIRCLE_SHADOW {
        coverage = gpu_sdf_shadow_coverage_from_dist(
            gpu_circle_sdf_distance(x - shadow_offset_x, y - shadow_offset_y, x0, y0, x1),
            shadow_expand,
            shadow_intensity,
        );
    } else if kind == CUBE_SDF_ARC {
        coverage =
            gpu_sdf_coverage_from_dist(gpu_arc_sdf_distance(x, y, x0, y0, x1, y1, r0, r1, r2));
    } else if kind == CUBE_SDF_ARC_SHADOW {
        coverage = gpu_sdf_shadow_coverage_from_dist(
            gpu_arc_sdf_distance(
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
    } else if kind == CUBE_SDF_CANDLESTICK {
        coverage = gpu_candlestick_sdf_coverage(x, y, x0, y0, x1, y1, r0, r1);
    } else if kind == CUBE_SDF_LINE {
        coverage = gpu_sdf_coverage_from_dist(gpu_line_sdf_distance(
            x, y, x0, y0, x1, y1, r0, r1,
        ));
    } else if kind == CUBE_SDF_DASH_LINE {
        coverage = gpu_sdf_coverage_from_dist(gpu_dash_line_sdf_distance(
            x, y, x0, y0, x1, y1, r0, r1, r2, r3, stroke_top,
        ));
    } else if kind == CUBE_SDF_LINE_SHADOW {
        coverage = gpu_sdf_shadow_coverage_from_dist(
            gpu_line_sdf_distance(
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
    (coverage * 255.0 + 0.5) as u32
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn gpu_dash_line_sdf_distance(
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
    let dash_length = dash_length_raw.max(0.0);
    let gap_length = gap_length_raw.max(0.0);
    let mut dist = gpu_line_sdf_distance(x, y, sx, sy, ex, ey, width, cap);
    if dash_length > f32::new(0.000001_f32) && gap_length > f32::new(0.000001_f32) {
        let half = width.max(0.0) * 0.5;
        let dx = ex - sx;
        let dy = ey - sy;
        let len = (dx * dx + dy * dy).sqrt();
        if len > f32::new(0.000001_f32) {
            let ux = dx / len;
            let uy = dy / len;
            let px = x - sx;
            let py = y - sy;
            let axis = px * ux + py * uy;
            let normal = -px * uy + py * ux;
            let cycle = dash_length + gap_length;
            let offset = gpu_rem_euclid_f32(dash_offset, cycle);
            let base = ((axis + offset) / cycle).floor();
            dist = gpu_dash_line_segment_distance(
                axis,
                normal,
                len,
                half,
                cap,
                dash_length,
                cycle,
                offset,
                base - 1.0,
            )
            .min(gpu_dash_line_segment_distance(
                axis,
                normal,
                len,
                half,
                cap,
                dash_length,
                cycle,
                offset,
                base,
            ))
            .min(gpu_dash_line_segment_distance(
                axis,
                normal,
                len,
                half,
                cap,
                dash_length,
                cycle,
                offset,
                base + 1.0,
            ));
        }
    }
    dist
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn gpu_dash_line_segment_distance(
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
    let mut dist = f32::new(1000000.0_f32);
    if dash_end > 0.0 && dash_start < len {
        let start = dash_start.max(0.0);
        let end = dash_end.min(len);
        if end > start {
            dist = gpu_line_segment_sdf_distance(axis, normal, start, end, half, cap);
        }
    }
    dist
}

#[cube]
fn gpu_line_segment_sdf_distance(
    axis: f32,
    normal: f32,
    start: f32,
    end: f32,
    half: f32,
    cap: f32,
) -> f32 {
    let nearest = axis.clamp(start, end);
    let mut dist = ((axis - nearest) * (axis - nearest) + normal * normal).sqrt() - half;
    if cap < 0.5 {
        dist = gpu_local_line_rect_distance(axis, normal, start, end, half);
    } else if cap < 1.5 {
        dist = gpu_local_line_rect_distance(axis, normal, start - half, end + half, half);
    }
    dist
}

#[cube]
fn gpu_candlestick_sdf_coverage(
    x: f32,
    y: f32,
    center_x: f32,
    high_y: f32,
    low_y: f32,
    body_top_y: f32,
    body_bottom_y: f32,
    body_width: f32,
) -> f32 {
    let wick = gpu_sdf_coverage_from_dist(gpu_rect_sdf_distance(
        x,
        y,
        center_x - 0.5,
        high_y.min(low_y),
        center_x + 0.5,
        high_y.max(low_y),
        0.0,
        0.0,
        0.0,
        0.0,
    ));
    let half_width = body_width.max(1.0) * 0.5;
    let mut body_y0 = body_top_y.min(body_bottom_y);
    let mut body_y1 = body_top_y.max(body_bottom_y);
    if body_y0 == body_y1 {
        body_y0 -= 0.5;
        body_y1 += 0.5;
    }
    let body = gpu_sdf_coverage_from_dist(gpu_rect_sdf_distance(
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
    wick.max(body)
}

#[cube]
fn gpu_line_sdf_distance(
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    ex: f32,
    ey: f32,
    width: f32,
    cap: f32,
) -> f32 {
    let half = width.max(0.0) * 0.5;
    let dx = ex - sx;
    let dy = ey - sy;
    let len = (dx * dx + dy * dy).sqrt();
    let mut dist = f32::new(1000000.0_f32);
    if len <= f32::new(0.000001_f32) {
        if cap >= 0.5 {
            if cap > 1.5 {
                dist = ((x - sx) * (x - sx) + (y - sy) * (y - sy)).sqrt() - half;
            } else {
                dist = gpu_local_line_rect_distance(0.0, 0.0, -half, half, half);
            }
        }
    } else {
        let ux = dx / len;
        let uy = dy / len;
        let px = x - sx;
        let py = y - sy;
        let axis = px * ux + py * uy;
        let normal = -px * uy + py * ux;
        if cap < 0.5 {
            dist = gpu_local_line_rect_distance(axis, normal, 0.0, len, half);
        } else if cap < 1.5 {
            dist = gpu_local_line_rect_distance(axis, normal, -half, len + half, half);
        } else {
            let nearest = axis.clamp(0.0, len);
            dist = ((axis - nearest) * (axis - nearest) + normal * normal).sqrt() - half;
        }
    }
    dist
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn gpu_arc_sdf_distance(
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
    let radius = radius_raw.max(0.0);
    let width = width_raw.max(0.0);
    let mut dist = f32::new(1000000.0_f32);
    if radius > f32::new(0.000001_f32)
        && width > f32::new(0.000001_f32)
        && sweep_angle.abs() > f32::new(0.000001_f32)
    {
        let vx = x - cx;
        let vy = y - cy;
        let len = (vx * vx + vy * vy).sqrt();
        let half = width * 0.5;
        if sweep_angle.abs() >= f32::new(6.283_085_3_f32) {
            dist = (len - radius).abs() - half;
        } else {
            let body = gpu_arc_butt_sdf_distance(vx, vy, len, radius, half, start_angle, sweep_angle);
            dist = body;
            if cap > 1.5 {
                dist = dist
                    .min(gpu_arc_endpoint_distance(vx, vy, radius, start_angle) - half)
                    .min(gpu_arc_endpoint_distance(vx, vy, radius, start_angle + sweep_angle) - half);
            } else if cap >= 0.5 {
                dist = dist
                    .min(gpu_arc_square_cap_distance(
                        vx,
                        vy,
                        radius,
                        start_angle,
                        sweep_angle,
                        -half,
                        0.0,
                        half,
                    ))
                    .min(gpu_arc_square_cap_distance(
                        vx,
                        vy,
                        radius,
                        start_angle + sweep_angle,
                        sweep_angle,
                        0.0,
                        half,
                        half,
                    ));
            }
        }
    }
    dist
}

#[cube]
fn gpu_arc_butt_sdf_distance(
    vx: f32,
    vy: f32,
    len: f32,
    radius: f32,
    half: f32,
    start_angle: f32,
    sweep_angle: f32,
) -> f32 {
    if len <= f32::new(0.000001_f32) {
        gpu_arc_endpoint_distance(vx, vy, radius, start_angle).min(gpu_arc_endpoint_distance(
            vx,
            vy,
            radius,
            start_angle + sweep_angle,
        )) - half
    } else {
        let angle = vy.atan2(vx);
        let radial = (len - radius).abs() - half;
        if gpu_arc_angle_in_sweep(angle, start_angle, sweep_angle) {
            radial
        } else {
            gpu_arc_cap_segment_distance(vx, vy, radius, start_angle, half).min(
                gpu_arc_cap_segment_distance(vx, vy, radius, start_angle + sweep_angle, half),
            )
        }
    }
}

#[cube]
fn gpu_arc_angle_in_sweep(angle: f32, start_angle: f32, sweep_angle: f32) -> bool {
    let tau = f32::new(6.283_185_5_f32);
    let eps = f32::new(0.000001_f32);
    if sweep_angle >= 0.0 {
        gpu_rem_euclid_f32(angle - start_angle, tau) <= sweep_angle + eps
    } else {
        gpu_rem_euclid_f32(start_angle - angle, tau) <= -sweep_angle + eps
    }
}

#[cube]
fn gpu_arc_endpoint_distance(vx: f32, vy: f32, radius: f32, angle: f32) -> f32 {
    let ex = radius * angle.cos();
    let ey = radius * angle.sin();
    ((vx - ex) * (vx - ex) + (vy - ey) * (vy - ey)).sqrt()
}

#[cube]
fn gpu_arc_cap_segment_distance(vx: f32, vy: f32, radius: f32, angle: f32, half: f32) -> f32 {
    let inner_radius = (radius - half).max(0.0);
    let outer_radius = radius + half;
    let co = angle.cos();
    let si = angle.sin();
    gpu_distance_to_segment(
        vx,
        vy,
        inner_radius * co,
        inner_radius * si,
        outer_radius * co,
        outer_radius * si,
    )
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn gpu_arc_square_cap_distance(
    vx: f32,
    vy: f32,
    radius: f32,
    angle: f32,
    sweep_angle: f32,
    x0: f32,
    x1: f32,
    half: f32,
) -> f32 {
    let mut dir = 1.0;
    if sweep_angle < 0.0 {
        dir = -1.0;
    }
    let co = angle.cos();
    let si = angle.sin();
    let ex = radius * co;
    let ey = radius * si;
    let tangent_x = -si * dir;
    let tangent_y = co * dir;
    let px = vx - ex;
    let py = vy - ey;
    let local_x = px * tangent_x + py * tangent_y;
    let local_y = -px * tangent_y + py * tangent_x;
    gpu_local_line_rect_distance(local_x, local_y, x0, x1, half)
}

#[cube]
fn gpu_distance_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    let mut dist = ((px - ax) * (px - ax) + (py - ay) * (py - ay)).sqrt();
    if len2 > f32::new(0.000001_f32) {
        let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
        let nx = ax + dx * t;
        let ny = ay + dy * t;
        dist = ((px - nx) * (px - nx) + (py - ny) * (py - ny)).sqrt();
    }
    dist
}

#[cube]
fn gpu_rem_euclid_f32(value: f32, modulus: f32) -> f32 {
    value - (value / modulus).floor() * modulus
}

#[cube]
fn gpu_local_line_rect_distance(axis: f32, normal: f32, x0: f32, x1: f32, half_height: f32) -> f32 {
    let center = (x0 + x1) * 0.5;
    let half_width = (x1 - x0) * 0.5;
    let dx = (axis - center).abs() - half_width;
    let dy = normal.abs() - half_height;
    (dx.max(0.0) * dx.max(0.0) + dy.max(0.0) * dy.max(0.0)).sqrt() + dx.max(dy).min(0.0)
}

#[cube]
fn gpu_sdf_coverage_from_dist(dist: f32) -> f32 {
    (f32::new(0.5_f32) - dist).clamp(0.0, 1.0)
}

#[cube]
fn gpu_sdf_shadow_coverage_from_dist(dist: f32, expand: f32, intensity: f32) -> f32 {
    let intensity = intensity.clamp(0.0, 1.0);
    let mut coverage = gpu_sdf_coverage_from_dist(dist) * intensity;
    if expand > 0.0 {
        coverage = (-dist.max(0.0) / expand).exp() * intensity;
    }
    coverage.clamp(0.0, 1.0)
}

#[cube]
fn gpu_circle_sdf_distance(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> f32 {
    let dx = x - cx;
    let dy = y - cy;
    (dx * dx + dy * dy).sqrt() - radius
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn gpu_rect_sdf_distance(
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
    let x0 = x0_raw.min(x1_raw);
    let y0 = y0_raw.min(y1_raw);
    let x1 = x0_raw.max(x1_raw);
    let y1 = y0_raw.max(y1_raw);
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = (x1 - x0) * 0.5;
    let hy = (y1 - y0) * 0.5;
    let px = x - cx;
    let py = y - cy;
    let mut radius = top_left;
    if px >= 0.0 {
        if py <= 0.0 {
            radius = top_right;
        } else {
            radius = bottom_right;
        }
    } else if py > 0.0 {
        radius = bottom_left;
    }
    let r = radius.min(hx).min(hy).max(0.0);
    let ax = px.abs();
    let ay = py.abs();

    if r <= 0.0 {
        let dx = ax - hx;
        let dy = ay - hy;
        (dx.max(0.0) * dx.max(0.0) + dy.max(0.0) * dy.max(0.0)).sqrt() + dx.max(dy).min(0.0)
    } else {
        let qx = ax - hx + r;
        let qy = ay - hy + r;
        qx.max(qy).min(0.0) + (qx.max(0.0) * qx.max(0.0) + qy.max(0.0) * qy.max(0.0)).sqrt() - r
    }
}
