const BRUSH_KIND_SOLID: u32 = 0u;
const BRUSH_KIND_LINEAR: u32 = 1u;
const BRUSH_KIND_RADIAL: u32 = 2u;
const BRUSH_KIND_SWEEP: u32 = 3u;
const BRUSH_KIND_FOUR_CORNER: u32 = 4u;
const BRUSH_KIND_PATTERN: u32 = 5u;
fn brush_extend_t(input_t: f32, mode: u32) -> f32 {
    var t = input_t;
    if mode == 0u {
        return clamp(t, 0.0, 1.0);
    }
    if mode == 1u {
        return t - floor(t);
    }
    t = t - floor(t * 0.5) * 2.0;
    return select(t, 2.0 - t, t > 1.0);
}

fn sample_brush_u32(brush: u32, px: vec2<f32>) -> u32 {
    let kind = brush_load_u32(brush);
    if kind == BRUSH_KIND_SOLID {
        return brush_load_u32(brush + 40u);
    }
    if kind == BRUSH_KIND_PATTERN {
        let local_px = vec2<f32>(
            brush_load_f32(brush + 56u) * px.x + brush_load_f32(brush + 64u) * px.y + brush_load_f32(brush + 72u),
            brush_load_f32(brush + 60u) * px.x + brush_load_f32(brush + 68u) * px.y + brush_load_f32(brush + 76u),
        );
        let width = brush_load_u32(brush + 40u);
        let height = brush_load_u32(brush + 44u);
        let x = u32(((i32(floor(local_px.x)) % i32(width)) + i32(width)) % i32(width));
        let y = u32(((i32(floor(local_px.y)) % i32(height)) + i32(height)) % i32(height));
        return brush_pack(brush_unpack(brush_load_u32(brush_load_u32(brush + 4u) + (y * width + x) * 4u)) * brush_load_f32(brush + 28u));
    }

    let start = vec2<f32>(brush_load_f32(brush + 12u), brush_load_f32(brush + 16u));
    let end = vec2<f32>(brush_load_f32(brush + 20u), brush_load_f32(brush + 24u));
    var t = 0.0;
    var valid = true;

    if kind == BRUSH_KIND_LINEAR {
        let delta = end - start;
        let denominator = dot(delta, delta);
        if denominator > 1e-12 {
            t = dot(px - start, delta) / denominator;
        }
    } else if kind == BRUSH_KIND_RADIAL {
        let local_px = vec2<f32>(
            brush_load_f32(brush + 56u) * px.x + brush_load_f32(brush + 64u) * px.y + brush_load_f32(brush + 72u),
            brush_load_f32(brush + 60u) * px.x + brush_load_f32(brush + 68u) * px.y + brush_load_f32(brush + 76u),
        );
        let q = local_px - start;
        let dc = end - start;
        let r0 = brush_load_f32(brush + 28u);
        let dr = brush_load_f32(brush + 32u) - r0;
        let a = dot(dc, dc) - dr * dr;
        let b = -2.0 * (dot(q, dc) + r0 * dr);
        let c = dot(q, q) - r0 * r0;
        if abs(a) <= 1e-6 {
            if abs(b) > 1e-6 {
                t = -c / b;
                valid = r0 + t * dr >= 0.0;
            } else {
                valid = false;
            }
        } else {
            let discriminant = b * b - 4.0 * a * c;
            if discriminant >= 0.0 {
                let root = sqrt(discriminant);
                let t0 = (-b - root) / (2.0 * a);
                let t1 = (-b + root) / (2.0 * a);
                let valid0 = r0 + t0 * dr >= 0.0;
                let valid1 = r0 + t1 * dr >= 0.0;
                t = max(t0, t1);
                if valid0 && !valid1 {
                    t = t0;
                }
                if !valid0 && valid1 {
                    t = t1;
                }
                valid = valid0 || valid1;
            } else {
                valid = false;
            }
        }
    } else if kind == BRUSH_KIND_SWEEP {
        let angle0 = brush_load_f32(brush + 28u);
        let angle1 = brush_load_f32(brush + 32u);
        var angle = atan2(px.y - start.y, px.x - start.x);
        let tau = 6.283185307179586;
        let span = angle1 - angle0;
        if abs(span) > 1e-12 {
            if span > 0.0 && angle < angle0 {
                angle += ceil((angle0 - angle) / tau) * tau;
            } else if span < 0.0 && angle > angle0 {
                angle -= ceil((angle - angle0) / tau) * tau;
            }
            t = (angle - angle0) / span;
        }
    } else if kind == BRUSH_KIND_FOUR_CORNER {
        let size = end - start;
        var uv = vec2<f32>(0.0);
        if abs(size.x) > 1e-12 {
            uv.x = clamp((px.x - start.x) / size.x, 0.0, 1.0);
        }
        if abs(size.y) > 1e-12 {
            uv.y = clamp((px.y - start.y) / size.y, 0.0, 1.0);
        }
        let top = mix(
            brush_unpack(brush_load_u32(brush + 40u)),
            brush_unpack(brush_load_u32(brush + 44u)),
            uv.x,
        );
        let bottom = mix(
            brush_unpack(brush_load_u32(brush + 52u)),
            brush_unpack(brush_load_u32(brush + 48u)),
            uv.x,
        );
        return brush_pack(mix(top, bottom, uv.y));
    } else {
        return 0u;
    }

    if !valid {
        return 0u;
    }
    let ramp_len = max(brush_load_u32(brush + 8u), 1u);
    let ramp_last = ramp_len - 1u;
    let ramp_t = brush_extend_t(t, brush_load_u32(brush + 36u)) * f32(ramp_last);
    let left_ix = min(u32(floor(ramp_t)), ramp_last);
    let right_ix = min(left_ix + 1u, ramp_last);
    let frac = ramp_t - f32(left_ix);
    if frac <= 1e-6 || left_ix == right_ix {
        return brush_load_u32(brush_load_u32(brush + 4u) + left_ix * 4u);
    }
    let left = brush_unpack(brush_load_u32(brush_load_u32(brush + 4u) + left_ix * 4u));
    let right = brush_unpack(brush_load_u32(brush_load_u32(brush + 4u) + right_ix * 4u));
    return brush_pack(mix(left, right, frac));
}
