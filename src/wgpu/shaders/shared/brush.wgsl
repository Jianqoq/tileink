fn sample_brush(brush_index: u32, x: f32, y: f32) -> u32 {
    let data_base = brush_index * GPU_BRUSH_U32_STRIDE;
    let kind = brush_data[data_base];
    let extend = brush_data[data_base + 1u];
    let payload_offset = brush_data[data_base + 2u];
    let payload_len = brush_data[data_base + 3u];
    let base = brush_index * GPU_BRUSH_PARAM_STRIDE;
    var color = brush_data[data_base + 4u];

    if (kind == GPU_BRUSH_LINEAR) {
        let tx = brush_params[base + 4u] * x + brush_params[base + 6u] * y + brush_params[base + 8u];
        let ty = brush_params[base + 5u] * x + brush_params[base + 7u] * y + brush_params[base + 9u];
        let sx = brush_params[base];
        let sy = brush_params[base + 1u];
        let ex = brush_params[base + 2u];
        let ey = brush_params[base + 3u];
        let dx = ex - sx;
        let dy = ey - sy;
        let denominator = dx * dx + dy * dy;
        var t = 0.0;
        if (denominator > 0.00000011920929) {
            t = ((tx - sx) * dx + (ty - sy) * dy) / denominator;
        }
        color = sample_ramp(payload_offset, payload_len, t, extend);
    } else if (kind == GPU_BRUSH_RADIAL) {
        color = sample_radial(x, y, base, extend, payload_offset, payload_len);
    } else if (kind == GPU_BRUSH_SWEEP) {
        let cx = brush_params[base];
        let cy = brush_params[base + 1u];
        let start_angle = brush_params[base + 2u];
        let end_angle = brush_params[base + 3u];
        let span = end_angle - start_angle;
        var t = 0.0;
        if (abs(span) > 0.00000011920929) {
            let tau = 6.2831855;
            var angle = atan2(y - cy, x - cx);
            if (span > 0.0) {
                while (angle < start_angle) {
                    angle = angle + tau;
                }
            } else {
                while (angle > start_angle) {
                    angle = angle - tau;
                }
            }
            t = (angle - start_angle) / span;
        }
        color = sample_ramp(payload_offset, payload_len, t, extend);
    } else if (kind == GPU_BRUSH_FOUR_CORNER) {
        color = sample_four_corner(x, y, base, payload_offset);
    } else if (kind == GPU_BRUSH_PATTERN) {
        color = sample_pattern(
            x,
            y,
            base,
            payload_offset,
            payload_len,
            brush_data[data_base + 5u],
            brush_data[data_base + 6u],
            brush_data[data_base + 7u],
            extend,
            brush_data[data_base + 8u],
        );
    } else if (kind == GPU_BRUSH_PATTERN_RESOURCE) {
        color = sample_resource_pattern(
            x,
            y,
            base,
            payload_offset,
            brush_data[data_base + 7u],
            extend,
            brush_data[data_base + 8u],
        );
    }

    return color;
}

fn sample_radial(x: f32, y: f32, base: u32, extend: u32, payload_offset: u32, payload_len: u32) -> u32 {
    let tx = brush_params[base + 6u] * x + brush_params[base + 8u] * y + brush_params[base + 10u];
    let ty = brush_params[base + 7u] * x + brush_params[base + 9u] * y + brush_params[base + 11u];
    let sx = brush_params[base];
    let sy = brush_params[base + 1u];
    let ex = brush_params[base + 2u];
    let ey = brush_params[base + 3u];
    let start_radius = brush_params[base + 4u];
    let end_radius = brush_params[base + 5u];
    let qx = tx - sx;
    let qy = ty - sy;
    let dcx = ex - sx;
    let dcy = ey - sy;
    let dr = end_radius - start_radius;
    let a = dcx * dcx + dcy * dcy - dr * dr;
    let b = -2.0 * (qx * dcx + qy * dcy + start_radius * dr);
    let c = qx * qx + qy * qy - start_radius * start_radius;
    var has_t = false;
    var t = 0.0;

    if (abs(a) <= 0.000001) {
        if (abs(b) > 0.000001) {
            let candidate = -c / b;
            if (start_radius + candidate * dr >= 0.0) {
                has_t = true;
                t = candidate;
            }
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if (discriminant >= 0.0) {
            let root = sqrt(discriminant);
            let t0 = (-b - root) / (2.0 * a);
            let t1 = (-b + root) / (2.0 * a);
            let valid0 = start_radius + t0 * dr >= 0.0;
            let valid1 = start_radius + t1 * dr >= 0.0;
            if (valid0) {
                has_t = true;
                if (valid1) {
                    t = max(t0, t1);
                } else {
                    t = t0;
                }
            } else if (valid1) {
                has_t = true;
                t = t1;
            }
        }
    }

    var color = 0u;
    if (has_t) {
        color = sample_ramp(payload_offset, payload_len, t, extend);
    }
    return color;
}

fn sample_four_corner(x: f32, y: f32, base: u32, payload_offset: u32) -> u32 {
    let x0 = brush_params[base];
    let y0 = brush_params[base + 1u];
    let x1 = brush_params[base + 2u];
    let y1 = brush_params[base + 3u];
    let width = x1 - x0;
    let height = y1 - y0;
    var u = 0.0;
    var v = 0.0;
    if (abs(width) > 0.00000011920929) {
        u = clamp((x - x0) / width, 0.0, 1.0);
    }
    if (abs(height) > 0.00000011920929) {
        v = clamp((y - y0) / height, 0.0, 1.0);
    }
    let tl = brush_payloads[payload_offset];
    let tr = brush_payloads[payload_offset + 1u];
    let br = brush_payloads[payload_offset + 2u];
    let bl = brush_payloads[payload_offset + 3u];
    let top = lerp_premul_u8(tl, tr, u);
    let bottom = lerp_premul_u8(bl, br, u);
    return lerp_premul_u8(top, bottom, v);
}

fn sample_pattern(
    x: f32,
    y: f32,
    base: u32,
    payload_offset: u32,
    payload_len: u32,
    width: u32,
    height: u32,
    opacity: u32,
    extend: u32,
    sampling: u32,
) -> u32 {
    var color = 0u;
    if (payload_len > 0u && width > 0u && height > 0u) {
        let tx = brush_params[base] * x + brush_params[base + 2u] * y + brush_params[base + 4u];
        let ty = brush_params[base + 1u] * x + brush_params[base + 3u] * y + brush_params[base + 5u];
        color = sample_pattern_pixels(false, tx, ty, payload_offset, payload_len, width, height, opacity, extend, sampling);
    }
    return color;
}

fn sample_resource_pattern(
    x: f32,
    y: f32,
    base: u32,
    resource_index: u32,
    opacity: u32,
    extend: u32,
    sampling: u32,
) -> u32 {
    let metadata_base = resource_index * 4u;
    let payload_offset = image_resource_metadata[metadata_base];
    let payload_len = image_resource_metadata[metadata_base + 1u];
    let width = image_resource_metadata[metadata_base + 2u];
    let height = image_resource_metadata[metadata_base + 3u];
    var color = 0u;
    if (payload_len > 0u && width > 0u && height > 0u) {
        let tx = (brush_params[base] * x + brush_params[base + 2u] * y + brush_params[base + 4u]) * f32(width);
        let ty = (brush_params[base + 1u] * x + brush_params[base + 3u] * y + brush_params[base + 5u]) * f32(height);
        color = sample_pattern_pixels(true, tx, ty, payload_offset, payload_len, width, height, opacity, extend, sampling);
    }
    return color;
}

fn sample_pattern_pixels(
    use_resource: bool,
    tx: f32,
    ty: f32,
    payload_offset: u32,
    payload_len: u32,
    width: u32,
    height: u32,
    opacity: u32,
    extend: u32,
    sampling: u32,
) -> u32 {
    var color = 0u;
    if (sampling == GPU_PATTERN_BILINEAR) {
        let sx = tx - 0.5;
        let sy = ty - 0.5;
        let x0f = floor(sx);
        let y0f = floor(sy);
        let fx = sx - x0f;
        let fy = sy - y0f;
        let x0 = i32(x0f);
        let y0 = i32(y0f);
        let tl = pattern_pixel(use_resource, payload_offset, payload_len, width, height, extend, x0, y0);
        let tr = pattern_pixel(use_resource, payload_offset, payload_len, width, height, extend, x0 + 1, y0);
        let bl = pattern_pixel(use_resource, payload_offset, payload_len, width, height, extend, x0, y0 + 1);
        let br = pattern_pixel(use_resource, payload_offset, payload_len, width, height, extend, x0 + 1, y0 + 1);
        color = lerp_premul_u8(lerp_premul_u8(tl, tr, fx), lerp_premul_u8(bl, br, fx), fy);
    } else {
        color = pattern_pixel(use_resource, payload_offset, payload_len, width, height, extend, i32(floor(tx)), i32(floor(ty)));
    }
    return scale_premul_u8(color, opacity);
}

fn pattern_pixel(use_resource: bool, payload_offset: u32, payload_len: u32, width: u32, height: u32, extend: u32, x: i32, y: i32) -> u32 {
    let local_x = extend_coord_i32(x, width, extend);
    let local_y = extend_coord_i32(y, height, extend);
    let local_ix = min(local_y * width + local_x, payload_len - 1u);
    if (use_resource) {
        return image_resource_pixels[payload_offset + local_ix];
    }
    return brush_payloads[payload_offset + local_ix];
}

fn sample_ramp(payload_offset: u32, payload_len: u32, t: f32, extend: u32) -> u32 {
    var color = 0u;
    if (payload_len > 0u) {
        let last = payload_len - 1u;
        let position = apply_extend(t, extend) * f32(last);
        let left_ix = u32(floor(position));
        let right_ix = min(left_ix + 1u, last);
        let frac = position - f32(left_ix);
        let left = brush_payloads[payload_offset + left_ix];
        let right = brush_payloads[payload_offset + right_ix];
        if (frac <= 0.00000011920929 || left_ix == right_ix) {
            color = left;
        } else {
            color = lerp_premul_u8(left, right, frac);
        }
    }
    return color;
}

fn apply_extend(t: f32, extend: u32) -> f32 {
    var out = clamp(t, 0.0, 1.0);
    if (extend == GPU_EXTEND_REPEAT) {
        out = rem_euclid_f32(t, 1.0);
    } else if (extend == GPU_EXTEND_REFLECT) {
        let value = rem_euclid_f32(t, 2.0);
        if (value <= 1.0) {
            out = value;
        } else {
            out = 2.0 - value;
        }
    }
    return out;
}

fn repeat_coord_i32(value: i32, size: u32) -> u32 {
    let size_i = i32(size);
    var out = value % size_i;
    if (out < 0) {
        out = out + size_i;
    }
    return u32(out);
}

fn extend_coord_i32(value: i32, size: u32, extend: u32) -> u32 {
    let max_coord = i32(size) - 1;
    var clamped = value;
    if (clamped < 0) {
        clamped = 0;
    }
    if (clamped > max_coord) {
        clamped = max_coord;
    }
    var out = u32(clamped);
    if (extend == GPU_EXTEND_REPEAT) {
        out = repeat_coord_i32(value, size);
    } else if (extend == GPU_EXTEND_REFLECT) {
        out = reflect_coord_i32(value, size);
    }
    return out;
}

fn reflect_coord_i32(value: i32, size: u32) -> u32 {
    var out = 0u;
    if (size > 1u) {
        let size_i = i32(size);
        let period = size_i * 2;
        var coord = value % period;
        if (coord < 0) {
            coord = coord + period;
        }
        if (coord < size_i) {
            out = u32(coord);
        } else {
            out = u32(period - coord - 1);
        }
    }
    return out;
}
