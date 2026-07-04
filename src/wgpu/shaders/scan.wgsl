struct ScanConfig {
    clear_len: u32,
    backdrop_len: u32,
    path_count: u32,
    scan_chunk_count: u32,
    line_count: u32,
    segment_capacity: u32,
    _pad0: u32,
    _pad1: u32,
};

struct Line {
    path_id: u32,
    _pad: f32,
    p0: vec2<f32>,
    p1: vec2<f32>,
};

struct PathRecord {
    path_id: u32,
    line_count: u32,
    line_start: u32,
    flags: u32,
    data_offset: u32,
    data_len: u32,
    tile_x0: u32,
    tile_y0: u32,
    tile_x1: u32,
    tile_y1: u32,
    segment_start: u32,
    segment_capacity: u32,
    segment_count: u32,
};

struct LineSegment {
    p0x: f32,
    p0y: f32,
    p1x: f32,
    p1y: f32,
    y_edge: f32,
};
struct TileSegmentRange {
    start: u32,
    end: u32,
};

@group(0) @binding(0) var<uniform> config: ScanConfig;
@group(0) @binding(1) var<storage, read> lines: array<Line>;
@group(0) @binding(2) var<storage, read> path_records: array<PathRecord>;
@group(0) @binding(3) var<storage, read> scan_chunk_backdrop_offsets: array<u32>;
@group(0) @binding(4) var<storage, read> scan_chunk_lens: array<u32>;
@group(0) @binding(5) var<storage, read> scan_chunk_range_starts: array<u32>;
@group(0) @binding(6) var<storage, read> scan_chunk_range_ends: array<u32>;
@group(0) @binding(7) var<storage, read_write> backdrops: array<atomic<i32>>;
@group(0) @binding(8) var<storage, read_write> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(10) var<storage, read_write> segment_tile_counts: array<atomic<u32>>;
@group(0) @binding(11) var<storage, read_write> segment_tile_cursors: array<atomic<u32>>;
@group(0) @binding(12) var<storage, read_write> segment_bumps: array<u32>;
@group(0) @binding(13) var<storage, read_write> chunk_totals: array<u32>;
@group(0) @binding(14) var<storage, read_write> chunk_offsets: array<u32>;
@group(0) @binding(15) var<storage, read_write> segments: array<LineSegment>;

const DDA_TOP_EDGE_EPSILON: f32 = 1.0e-5;
const TILE_BOUNDARY_EPSILON: f32 = 1.0e-4;

var<workgroup> scan_scratch: array<u32, 256>;

@compute @workgroup_size(256)
fn scan_clear(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let ix = global_id.x;
    if (ix >= config.clear_len) {
        return;
    }
    if (ix < config.backdrop_len) {
        atomicStore(&backdrops[ix], 0i);
        segment_ranges[ix].start = 0u;
        segment_ranges[ix].end = 0u;
        atomicStore(&segment_tile_counts[ix], 0u);
        atomicStore(&segment_tile_cursors[ix], 0u);
    }
    if (ix < config.path_count) {
        segment_bumps[ix] = 0u;
    }
    if (ix < config.scan_chunk_count) {
        chunk_totals[ix] = 0u;
        chunk_offsets[ix] = 0u;
    }
}

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
                top_x - left_x >= -TILE_BOUNDARY_EPSILON &&
                top_x - left_x <= TILE_BOUNDARY_EPSILON &&
                left_y - top_y >= -TILE_BOUNDARY_EPSILON &&
                left_y - top_y <= TILE_BOUNDARY_EPSILON
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
        s0y < f32(bbox_y0) - TILE_BOUNDARY_EPSILON &&
        s1y > f32(bbox_y0) + TILE_BOUNDARY_EPSILON
    ) {
        let top_y = f32(bbox_y0);
        let top_x = s0x + (s1x - s0x) * ((top_y - s0y) / (s1y - s0y));
        if (top_x >= f32(bbox_x0) - TILE_BOUNDARY_EPSILON && top_x < f32(bbox_x1)) {
            var x_bump = i32(ceil(top_x - TILE_BOUNDARY_EPSILON));
            if (top_x - f32(bbox_x0) <= TILE_BOUNDARY_EPSILON) {
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
                initial_top_edge = imin == 0u && abs(y0 - xy0y * tile_scale) <= DDA_TOP_EDGE_EPSILON;
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

@compute @workgroup_size(256)
fn scan_prefix_chunks(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let chunk_ix = workgroup_id.x;
    let lane = local_id.x;
    let chunk_offset = scan_chunk_backdrop_offsets[chunk_ix];
    let chunk_len = scan_chunk_lens[chunk_ix];

    var count = 0u;
    if (lane < chunk_len) {
        count = atomicLoad(&segment_tile_counts[chunk_offset + lane]);
    }
    scan_scratch[lane] = count;
    workgroupBarrier();

    var step = 1u;
    loop {
        if (step >= 256u) {
            break;
        }
        var add = 0u;
        if (lane >= step) {
            add = scan_scratch[lane - step];
        }
        workgroupBarrier();
        if (lane >= step) {
            scan_scratch[lane] += add;
        }
        workgroupBarrier();
        step *= 2u;
    }

    if (lane < chunk_len) {
        let inclusive = scan_scratch[lane];
        let exclusive = inclusive - count;
        let ix = chunk_offset + lane;
        segment_ranges[ix].start = exclusive;
        segment_ranges[ix].end = inclusive;
    }
    if (lane + 1u == chunk_len) {
        chunk_totals[chunk_ix] = scan_scratch[lane];
    }
}

@compute @workgroup_size(256)
fn scan_chunk_offsets(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let path_id = global_id.x;
    if (path_id >= config.path_count) {
        return;
    }
    let path = path_records[path_id];
    var next = path.segment_start;
    var chunk_ix = scan_chunk_range_starts[path_id];
    let chunk_end = scan_chunk_range_ends[path_id];
    loop {
        if (chunk_ix >= chunk_end) {
            break;
        }
        chunk_offsets[chunk_ix] = next;
        next += chunk_totals[chunk_ix];
        chunk_ix += 1u;
    }
    segment_bumps[path_id] = next - path.segment_start;
}

@compute @workgroup_size(256)
fn scan_apply_chunk_offsets(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let chunk_ix = workgroup_id.x;
    let lane = local_id.x;
    let chunk_len = scan_chunk_lens[chunk_ix];
    if (lane >= chunk_len) {
        return;
    }

    let ix = scan_chunk_backdrop_offsets[chunk_ix] + lane;
    let base = chunk_offsets[chunk_ix];
    let start = segment_ranges[ix].start + base;
    let end = segment_ranges[ix].end + base;
    segment_ranges[ix].start = start;
    segment_ranges[ix].end = end;
    atomicStore(&segment_tile_cursors[ix], start);
}

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

    if (max(s0x, s1x) <= f32(bbox_x0)) {
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
                    keep_horizontal_tile_edges,
                    tile_x,
                    tile_y
                );
            }
        }
        i += 1u;
    }
}

fn span(a: f32, b: f32) -> u32 {
    var hi = ceil(a);
    if (b > a) {
        hi = ceil(b);
    }
    var lo = floor(a);
    if (b < a) {
        lo = floor(b);
    }
    var value = hi - lo;
    if (value < 1.0) {
        value = 1.0;
    }
    return u32(value);
}

fn local_tile_ix(tile_x: i32, tile_y: i32, bbox_x0: u32, bbox_y0: u32, bbox_x1: u32) -> u32 {
    let local_x = u32(tile_x) - bbox_x0;
    let local_y = u32(tile_y) - bbox_y0;
    return local_y * (bbox_x1 - bbox_x0) + local_x;
}

fn write_clipped_segment(
    dst: u32,
    line_x0: f32,
    line_y0: f32,
    line_x1: f32,
    line_y1: f32,
    is_down: bool,
    keep_horizontal_tile_edges: bool,
    tile_x: i32,
    tile_y: i32,
) {
    let tile_size = 16.0;
    let tile_min_x = f32(tile_x) * tile_size;
    let tile_min_y = f32(tile_y) * tile_size;
    let tile_max_x = tile_min_x + tile_size;
    let tile_max_y = tile_min_y + tile_size;

    let dx = line_x1 - line_x0;
    let dy = line_y1 - line_y0;
    var t0 = 0.0;
    var t1 = 1.0;
    let clip_left = 1u;
    let clip_right = 2u;
    let clip_top = 4u;
    let clip_bottom = 8u;
    var t0_clip = 0u;
    var t1_clip = 0u;
    var valid = true;

    var p = -dx;
    var q = line_x0 - tile_min_x;
    if (p == 0.0) {
        if (q < 0.0) {
            valid = false;
        }
    } else {
        let r = q / p;
        if (p < 0.0) {
            if (r > t1) {
                valid = false;
            } else if (r > t0) {
                t0 = r;
                t0_clip = clip_left;
            } else if (r == t0) {
                t0_clip |= clip_left;
            }
        } else if (r < t0) {
            valid = false;
        } else if (r < t1) {
            t1 = r;
            t1_clip = clip_left;
        } else if (r == t1) {
            t1_clip |= clip_left;
        }
    }

    p = dx;
    q = tile_max_x - line_x0;
    if (p == 0.0) {
        if (q < 0.0) {
            valid = false;
        }
    } else {
        let r = q / p;
        if (p < 0.0) {
            if (r > t1) {
                valid = false;
            } else if (r > t0) {
                t0 = r;
                t0_clip = clip_right;
            } else if (r == t0) {
                t0_clip |= clip_right;
            }
        } else if (r < t0) {
            valid = false;
        } else if (r < t1) {
            t1 = r;
            t1_clip = clip_right;
        } else if (r == t1) {
            t1_clip |= clip_right;
        }
    }

    p = -dy;
    q = line_y0 - tile_min_y;
    if (p == 0.0) {
        if (q < 0.0) {
            valid = false;
        }
    } else {
        let r = q / p;
        if (p < 0.0) {
            if (r > t1) {
                valid = false;
            } else if (r > t0) {
                t0 = r;
                t0_clip = clip_top;
            } else if (r == t0) {
                t0_clip |= clip_top;
            }
        } else if (r < t0) {
            valid = false;
        } else if (r < t1) {
            t1 = r;
            t1_clip = clip_top;
        } else if (r == t1) {
            t1_clip |= clip_top;
        }
    }

    p = dy;
    q = tile_max_y - line_y0;
    if (p == 0.0) {
        if (q < 0.0) {
            valid = false;
        }
    } else {
        let r = q / p;
        if (p < 0.0) {
            if (r > t1) {
                valid = false;
            } else if (r > t0) {
                t0 = r;
                t0_clip = clip_bottom;
            } else if (r == t0) {
                t0_clip |= clip_bottom;
            }
        } else if (r < t0) {
            valid = false;
        } else if (r < t1) {
            t1 = r;
            t1_clip = clip_bottom;
        } else if (r == t1) {
            t1_clip |= clip_bottom;
        }
    }

    var xy0x = clamp(line_x0, tile_min_x, tile_max_x);
    var xy0y = clamp(line_y0, tile_min_y, tile_max_y);
    var xy1x = clamp(line_x1, tile_min_x, tile_max_x);
    var xy1y = clamp(line_y1, tile_min_y, tile_max_y);
    if (valid) {
        xy0x = line_x0 + dx * t0;
        xy0y = line_y0 + dy * t0;
        xy1x = line_x0 + dx * t1;
        xy1y = line_y0 + dy * t1;
        if ((t0_clip & clip_left) != 0u) {
            xy0x = tile_min_x;
        }
        if ((t0_clip & clip_right) != 0u) {
            xy0x = tile_max_x;
        }
        if ((t0_clip & clip_top) != 0u) {
            xy0y = tile_min_y;
        }
        if ((t0_clip & clip_bottom) != 0u) {
            xy0y = tile_max_y;
        }
        if ((t1_clip & clip_left) != 0u) {
            xy1x = tile_min_x;
        }
        if ((t1_clip & clip_right) != 0u) {
            xy1x = tile_max_x;
        }
        if ((t1_clip & clip_top) != 0u) {
            xy1y = tile_min_y;
        }
        if ((t1_clip & clip_bottom) != 0u) {
            xy1y = tile_max_y;
        }
    }

    var y_edge = 1000000000.0;
    var p0x = clamp(xy0x - tile_min_x, 0.0, tile_size);
    var p0y = clamp(xy0y - tile_min_y, 0.0, tile_size);
    var p1x = clamp(xy1x - tile_min_x, 0.0, tile_size);
    var p1y = clamp(xy1y - tile_min_y, 0.0, tile_size);
    let boundary_epsilon = TILE_BOUNDARY_EPSILON;
    if (p0x <= boundary_epsilon) {
        p0x = 0.0;
    } else if (tile_size - p0x <= boundary_epsilon) {
        p0x = tile_size;
    }
    if (p0y <= boundary_epsilon) {
        p0y = 0.0;
    } else if (tile_size - p0y <= boundary_epsilon) {
        p0y = tile_size;
    }
    if (p1x <= boundary_epsilon) {
        p1x = 0.0;
    } else if (tile_size - p1x <= boundary_epsilon) {
        p1x = tile_size;
    }
    if (p1y <= boundary_epsilon) {
        p1y = 0.0;
    } else if (tile_size - p1y <= boundary_epsilon) {
        p1y = tile_size;
    }
    let epsilon = 0.000001;

    if (p0x == 0.0) {
        if (p1x == 0.0) {
            p0x = epsilon;
            if (p0y == 0.0) {
                p1x = epsilon;
                p1y = tile_size;
            } else {
                p1x = 2.0 * epsilon;
                p1y = p0y;
            }
        } else if (p0y == 0.0) {
            if (
                (keep_horizontal_tile_edges && p1y == 0.0) ||
                (p1x <= 1.0 + boundary_epsilon && p1y <= 1.0 + boundary_epsilon)
            ) {
                y_edge = p0y;
            }
            p0x = epsilon;
        } else {
            y_edge = p0y;
        }
    } else if (p1x == 0.0) {
        if (p1y == 0.0) {
            if (keep_horizontal_tile_edges && p0y == 0.0) {
                y_edge = p1y;
            }
            p1x = epsilon;
        } else {
            y_edge = p1y;
        }
    }
    if (floor(p0x) == p0x && p0x != 0.0) {
        p0x -= epsilon;
    }
    if (floor(p1x) == p1x && p1x != 0.0) {
        p1x -= epsilon;
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
