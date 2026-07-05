#include "common.wgsl"

@group(0) @binding(1) var<storage, read> lines: array<Line>;
@group(0) @binding(2) var<storage, read> path_records: array<PathRecord>;
@group(0) @binding(7) var<storage, read_write> backdrops: array<atomic<i32>>;
@group(0) @binding(10) var<storage, read_write> segment_tile_counts: array<atomic<u32>>;

@compute @workgroup_size(256)
fn scan_count(@builtin(global_invocation_id) global_id: vec3<u32>) {
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

    let keep_horizontal_tile_edges = path.flags >= 1u;
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
    if (dx + dy == 0.0 || (dy == 0.0 && floor(s0y) == s0y && !keep_horizontal_tile_edges)) {
        return;
    }

    let line_needs_top_edge_carry = !keep_horizontal_tile_edges ||
        (s0y != s1y && (floor(s0y) < floor(s1y) || bbox_y1 > bbox_y0 + 1u));
    let skip_initial_top_edge_carry = keep_horizontal_tile_edges && bbox_y0 == 0u && xy1x < xy0x;

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
    if (s0y >= f32(bbox_y1) || s1y < f32(bbox_y0) || xmin >= f32(bbox_x1)) {
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

    var delta = 1i;
    if (is_down) {
        delta = -1i;
    }
    var ymin = 0i;
    var ymax = 0i;
    if (max(s0x, s1x) <= f32(bbox_x0)) {
        ymin = i32(ceil(s0y));
        ymax = i32(ceil(s1y));
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
            let ynext = i32(y0 + f - floor(a * f + b) + 1.0);
            if (is_positive_slope) {
                if (u32(f) > imin) {
                    var ystart = y0 + 1.0;
                    if (y0 == s0y) {
                        ystart = y0;
                    }
                    ymin = i32(ystart);
                    ymax = ynext;
                    imin = u32(f);
                }
            } else if (u32(f) < imax) {
                ymin = ynext;
                ymax = i32(ceil(s1y));
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
    ymin = max(ymin, i32(bbox_y0));
    ymax = min(ymax, i32(bbox_y1));
    if (ymin == i32(bbox_y0) && ymax > ymin && s0y < f32(bbox_y0) && s1y > f32(bbox_y0)) {
        let dx_left = s1x - s0x;
        if (dx_left != 0.0) {
            let left_x = f32(bbox_x0);
            let top_y = f32(bbox_y0);
            let top_x = s0x + (s1x - s0x) * ((top_y - s0y) / (s1y - s0y));
            let left_y = s0y + (s1y - s0y) * ((left_x - s0x) / dx_left);
            if (
                top_x - left_x >= -SCAN_EPSILON &&
                top_x - left_x <= SCAN_EPSILON &&
                left_y - top_y >= -SCAN_EPSILON &&
                left_y - top_y <= SCAN_EPSILON
            ) {
                ymin += 1i;
            }
        }
    }

    let data_offset = path.data_offset;
    var y = ymin;
    loop {
        if (y >= ymax) {
            break;
        }
        let local = u32((y - i32(bbox_y0)) * i32(bbox_stride));
        atomicAdd(&backdrops[data_offset + local], delta);
        y += 1i;
    }
    if (
        imin < imax &&
        s0y < f32(bbox_y0) - SCAN_EPSILON &&
        s1y > f32(bbox_y0) + SCAN_EPSILON
    ) {
        let top_y = f32(bbox_y0);
        let top_x = s0x + (s1x - s0x) * ((top_y - s0y) / (s1y - s0y));
        if (top_x >= f32(bbox_x0) - SCAN_EPSILON && top_x < f32(bbox_x1)) {
            var x_bump = i32(ceil(top_x - SCAN_EPSILON));
            if (top_x - f32(bbox_x0) <= SCAN_EPSILON) {
                x_bump = i32(bbox_x0) + 1i;
            }
            if (x_bump >= i32(bbox_x0) && x_bump < i32(bbox_x1)) {
                let bump_local = u32(x_bump - i32(bbox_x0));
                atomicAdd(&backdrops[data_offset + bump_local], delta);
            }
        }
    }

    var last_z = floor(a * (f32(imin) - 1.0) + b);
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
            var top_edge = last_z == z;
            var initial_top_edge = false;
            if (i == imin) {
                initial_top_edge = imin == 0u && abs(y0 - xy0y * tile_scale) <= SCAN_EPSILON;
                top_edge = initial_top_edge;
            }
            if (
                line_needs_top_edge_carry &&
                top_edge &&
                !(initial_top_edge && skip_initial_top_edge_carry) &&
                tile_x + 1i < i32(bbox_x1)
            ) {
                let x_bump = max(tile_x + 1i, i32(bbox_x0));
                let bump_local = u32(
                    (tile_y - i32(bbox_y0)) * i32(bbox_stride) + x_bump - i32(bbox_x0)
                );
                atomicAdd(&backdrops[data_offset + bump_local], delta);
            }

            let local_ix = local_tile_ix(tile_x, tile_y, bbox_x0, bbox_y0, bbox_x1);
            atomicAdd(&segment_tile_counts[data_offset + local_ix], 1u);
        }
        last_z = z;
        i += 1u;
    }
}
