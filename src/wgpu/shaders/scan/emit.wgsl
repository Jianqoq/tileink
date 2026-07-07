#include "common.wgsl"

@group(0) @binding(1) var<storage, read> lines: array<Line>;
@group(0) @binding(2) var<storage, read> path_records: array<PathRecord>;
@group(0) @binding(3) var<storage, read_write> segment_tile_cursors: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read_write> segments: array<LineSegment>;

@compute @workgroup_size(256)
fn scan_emit(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let line_ix = global_id.x;
    if (line_ix >= config.line_count) {
        return;
    }
    let line = lines[line_ix];
    let path_id = line.path_id;
    if (path_id >= arrayLength(&path_records)) {
        return;
    }
    let path = path_records[path_id];

    let bbox_x0 = path.tile_x0;
    let bbox_y0 = path.tile_y0;
    let bbox_x1 = path.tile_x1;
    let bbox_y1 = path.tile_y1;
    let bbox_stride = bbox_x1 - bbox_x0;
    if (bbox_stride == 0u || bbox_y0 >= bbox_y1) {
        return;
    }

    let p0x = line.p0.x;
    let p0y = line.p0.y;
    let p1x = line.p1.x;
    let p1y = line.p1.y;
    let is_down = p1y >= p0y;
    var xy0x = p0x;
    var xy0y = p0y;
    var xy1x = p1x;
    var xy1y = p1y;
    if (!is_down) {
        xy0x = p1x;
        xy0y = p1y;
        xy1x = p0x;
        xy1y = p0y;
    }

    let tile_scale = 0.0625;
    let s0x = xy0x * tile_scale;
    let s0y = xy0y * tile_scale;
    let s1x = xy1x * tile_scale;
    let s1y = xy1y * tile_scale;
    let count_x = span(s0x, s1x) - 1u;
    let count = count_x + span(s0y, s1y);
    let dx = abs(s1x - s0x);
    let dy = s1y - s0y;
    if (dx + dy == 0.0 || (dy == 0.0 && floor(s0y) == s0y)) {
        return;
    }

    let idxdy = 1.0 / (dx + dy);
    var a = dx * idxdy;
    let is_positive_slope = s1x >= s0x;
    var sign = -1.0;
    if (is_positive_slope) {
        sign = 1.0;
    }
    let xt0 = floor(s0x * sign);
    let c = s0x * sign - xt0;
    let y0 = floor(s0y);
    var ytop = y0 + 1.0;
    if (s0y == s1y) {
        ytop = ceil(s0y);
    }
    let b = min((dy * c + dx * (ytop - s0y)) * idxdy, 0.99999994);
    let robust_err = floor(a * (f32(count) - 1.0) + b) - f32(count_x);
    if (robust_err != 0.0) {
        if (robust_err > 0.0) {
            a -= 0.0000002;
        } else {
            a += 0.0000002;
        }
    }
    var x0 = xt0 * sign - 1.0;
    if (is_positive_slope) {
        x0 = xt0 * sign;
    }
    let xmin = min(s0x, s1x);
    if (s0y >= f32(bbox_y1) || s1y <= f32(bbox_y0) + TOP_TOUCH_EPSILON || xmin >= f32(bbox_x1)) {
        return;
    }

    var imin = 0u;
    if (s0y < f32(bbox_y0)) {
        var iminf = round((f32(bbox_y0) - y0 + b - a) / (1.0 - a)) - 1.0;
        if (y0 + iminf - floor(a * iminf + b) < f32(bbox_y0)) {
            iminf += 1.0;
        }
        imin = u32(iminf);
    }
    var imax = count;
    if (s1y > f32(bbox_y1)) {
        var imaxf = round((f32(bbox_y1) - y0 + b - a) / (1.0 - a)) - 1.0;
        if (y0 + imaxf - floor(a * imaxf + b) < f32(bbox_y1)) {
            imaxf += 1.0;
        }
        imax = u32(imaxf);
    }

    if (max(s0x, s1x) < f32(bbox_x0)) {
        imax = imin;
    } else {
        var fudge = 1.0;
        if (is_positive_slope) {
            fudge = 0.0;
        }
        if (xmin < f32(bbox_x0)) {
            var f = round((sign * (f32(bbox_x0) - x0) - b + fudge) / a);
            if ((x0 + sign * floor(a * f + b) < f32(bbox_x0)) == is_positive_slope) {
                f += 1.0;
            }
            if (is_positive_slope) {
                if (u32(f) > imin) {
                    imin = u32(f);
                }
            } else if (u32(f) < imax) {
                imax = u32(f);
            }
        }
        if (max(s0x, s1x) > f32(bbox_x1)) {
            var f = round((sign * (f32(bbox_x1) - x0) - b + fudge) / a);
            if ((x0 + sign * floor(a * f + b) < f32(bbox_x1)) == is_positive_slope) {
                f += 1.0;
            }
            if (is_positive_slope) {
                imax = min(imax, u32(f));
            } else {
                imin = max(imin, u32(f));
            }
        }
    }
    imax = max(imin, imax);

    let data_offset = path.data_offset;
    var i = imin;
    loop {
        if (i >= imax) {
            break;
        }
        let z = floor(a * f32(i) + b);
        let tile_y = i32(y0 + f32(i) - z);
        let tile_x = i32(x0 + sign * z);
        if (
            tile_y >= i32(bbox_y0) &&
            tile_y < i32(bbox_y1) &&
            tile_x >= i32(bbox_x0) &&
            tile_x < i32(bbox_x1)
        ) {
            let local_ix = local_tile_ix(tile_x, tile_y, bbox_x0, bbox_y0, bbox_x1);
            let dst = atomicAdd(&segment_tile_cursors[data_offset + local_ix], 1u);
            if (dst < config.segment_capacity) {
                write_clipped_segment(
                    dst,
                    xy0x,
                    xy0y,
                    xy1x,
                    xy1y,
                    is_down,
                    count,
                    a,
                    b,
                    sign,
                    i,
                    z,
                    tile_x,
                    tile_y
                );
            }
        }
        i += 1u;
    }
}

fn write_clipped_segment(
    dst: u32,
    line_x0: f32,
    line_y0: f32,
    line_x1: f32,
    line_y1: f32,
    is_down: bool,
    count: u32,
    a: f32,
    b: f32,
    sign: f32,
    sub_ix: u32,
    z: f32,
    tile_x: i32,
    tile_y: i32,
) {
    let tile_size = 16.0;
    let tile_min_x = f32(tile_x) * tile_size;
    let tile_min_y = f32(tile_y) * tile_size;
    let tile_max_x = tile_min_x + tile_size;
    let tile_max_y = tile_min_y + tile_size;

    var xy0x = line_x0;
    var xy0y = line_y0;
    var xy1x = line_x1;
    var xy1y = line_y1;
    if (sub_ix > 0u) {
        let z_prev = floor(a * (f32(sub_ix) - 1.0) + b);
        if (z == z_prev) {
            var x = x_at_y(line_x0, line_y0, line_x1, line_y1, tile_min_y);
            x = clamp(x, tile_min_x + TILE_CLIP_NUDGE, tile_max_x);
            xy0x = x;
            xy0y = tile_min_y;
        } else {
            var x_clip = tile_max_x;
            if (sign > 0.0) {
                x_clip = tile_min_x;
            }
            let y = clip_y_at_x(line_x0, line_y0, line_x1, line_y1, x_clip, tile_min_y, tile_max_y);
            xy0x = x_clip;
            xy0y = y;
        }
    }
    if (sub_ix < count - 1u) {
        let z_next = floor(a * (f32(sub_ix) + 1.0) + b);
        if (z == z_next) {
            var x = x_at_y(line_x0, line_y0, line_x1, line_y1, tile_max_y);
            x = clamp(x, tile_min_x + TILE_CLIP_NUDGE, tile_max_x);
            xy1x = x;
            xy1y = tile_max_y;
        } else {
            var x_clip = tile_min_x;
            if (sign > 0.0) {
                x_clip = tile_max_x;
            }
            let y = clip_y_at_x(line_x0, line_y0, line_x1, line_y1, x_clip, tile_min_y, tile_max_y);
            xy1x = x_clip;
            xy1y = y;
        }
    }

    var y_edge = 1000000000.0;
    var p0x = clamp(xy0x - tile_min_x, 0.0, tile_size);
    var p0y = clamp(xy0y - tile_min_y, 0.0, tile_size);
    var p1x = clamp(xy1x - tile_min_x, 0.0, tile_size);
    var p1y = clamp(xy1y - tile_min_y, 0.0, tile_size);
    if (p0x <= SCAN_EPSILON) {
        p0x = 0.0;
    } else if (tile_size - p0x <= SCAN_EPSILON) {
        p0x = tile_size;
    }
    if (p0y <= SCAN_EPSILON) {
        p0y = 0.0;
    } else if (tile_size - p0y <= SCAN_EPSILON) {
        p0y = tile_size;
    }
    if (p1x <= SCAN_EPSILON) {
        p1x = 0.0;
    } else if (tile_size - p1x <= SCAN_EPSILON) {
        p1x = tile_size;
    }
    if (p1y <= SCAN_EPSILON) {
        p1y = 0.0;
    } else if (tile_size - p1y <= SCAN_EPSILON) {
        p1y = tile_size;
    }

    if (p0x == 0.0) {
        if (p1x == 0.0) {
            p0x = SCAN_EPSILON;
            if (p0y == 0.0) {
                p1x = SCAN_EPSILON;
                p1y = tile_size;
            } else {
                p1x = 2.0 * SCAN_EPSILON;
                p1y = p0y;
            }
        } else if (p0y == 0.0) {
            p0x = SCAN_EPSILON;
        } else {
            y_edge = p0y;
        }
    } else if (p1x == 0.0) {
        if (p1y == 0.0) {
            p1x = SCAN_EPSILON;
        } else {
            y_edge = p1y;
        }
    }
    if (floor(p0x) == p0x && p0x != 0.0) {
        p0x -= SCAN_EPSILON;
    }
    if (floor(p1x) == p1x && p1x != 0.0) {
        p1x -= SCAN_EPSILON;
    }
    if (!is_down) {
        let tmp_x = p0x;
        let tmp_y = p0y;
        p0x = p1x;
        p0y = p1y;
        p1x = tmp_x;
        p1y = tmp_y;
    }

    segments[dst] = LineSegment(p0x, p0y, p1x, p1y, y_edge);
}

fn x_at_y(x0: f32, y0: f32, x1: f32, y1: f32, y: f32) -> f32 {
    return x0 + (x1 - x0) * ((y - y0) / (y1 - y0));
}

fn y_at_x(x0: f32, y0: f32, x1: f32, y1: f32, x: f32) -> f32 {
    return y0 + (y1 - y0) * ((x - x0) / (x1 - x0));
}

fn clip_y_at_x(x0: f32, y0: f32, x1: f32, y1: f32, x: f32, tile_min_y: f32, tile_max_y: f32) -> f32 {
    let y = y_at_x(x0, y0, x1, y1, x);
    if (y <= tile_min_y + SCAN_EPSILON) {
        let top_x = x_at_y(x0, y0, x1, y1, tile_min_y);
        if (abs(top_x - x) > TILE_CLIP_NUDGE) {
            return tile_min_y;
        }
        return tile_min_y + TILE_CLIP_NUDGE;
    }
    return clamp(y, tile_min_y + TILE_CLIP_NUDGE, tile_max_y);
}
