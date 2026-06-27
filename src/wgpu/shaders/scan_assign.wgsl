// Scan assign: one workgroup per path, lanes stride over lines.
// Matches `src/gpu/cpu/scan.rs` + `src/cpu/scan_assign.rs`.

struct ScanParams {
    path_count: u32,
    width_in_tiles: u32,
    workgroup_count_x: u32,
    _pad: u32,
}

struct PathRecord {
    data_offset: u32,
    byte_len: u32,
    path_ix: u32,
    tolerance: f32,
    line_count: u32,
    line_start: u32,
}

struct BackdropRecord {
    path_ix: u32,
    data_offset: u32,
    tile_x0: u32,
    tile_y0: u32,
    tile_x1: u32,
    tile_y1: u32,
    segment_start: u32,
    segment_capacity: u32,
    segment_count: u32,
}

struct Line {
    path_ix: u32,
    _pad: u32,
    p0: vec2f,
    p1: vec2f,
}

struct TileSegment {
    path_ix: u32,
    tile_ix: u32,
    y_edge: f32,
    point0_x: f32,
    point0_y: f32,
    point1_x: f32,
    point1_y: f32,
}

struct TileBbox {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

struct ScanLinePlan {
    xy0: vec2f,
    xy1: vec2f,
    is_down: bool,
    is_positive_slope: bool,
    a: f32,
    b: f32,
    x0: f32,
    line_sign: f32,
    y0: f32,
    delta: i32,
    imin: u32,
    imax: u32,
    ymin: i32,
    ymax: i32,
    valid: bool,
}

@group(0) @binding(0)
var<uniform> params: ScanParams;

@group(0) @binding(1)
var<storage, read_write> paths: array<PathRecord>;

@group(0) @binding(2)
var<storage, read_write> lines: array<Line>;

@group(0) @binding(3)
var<storage, read_write> backdrops: array<BackdropRecord>;

@group(0) @binding(4)
var<storage, read_write> backdrop_pool: array<atomic<i32>>;

@group(0) @binding(5)
var<storage, read_write> segments: array<TileSegment>;

@group(0) @binding(6)
var<storage, read_write> starts: array<atomic<u32>>;

@group(0) @binding(7)
var<storage, read_write> cursors: array<atomic<u32>>;

const WORKGROUP_SIZE: u32 = 256u;
const TILE_SIZE: f32 = 16.0;
const TILE_SCALE: f32 = 1.0 / TILE_SIZE;

fn tile_bbox(bd: BackdropRecord) -> TileBbox {
    return TileBbox(
        i32(bd.tile_x0),
        i32(bd.tile_y0),
        i32(bd.tile_x1),
        i32(bd.tile_y1),
    );
}

fn span(a: f32, b: f32) -> u32 {
    let hi = ceil(max(a, b));
    let lo = floor(min(a, b));
    return u32(max(hi - lo, 1.0));
}

fn plan_scan_line(line: Line, bbox: TileBbox) -> ScanLinePlan {
    var invalid = ScanLinePlan(
        vec2f(0.0), vec2f(0.0), false, false,
        0.0, 0.0, 0.0, 0.0, 0.0, 0, 0u, 0u, 0, 0, false,
    );

    let p0 = line.p0;
    let p1 = line.p1;
    let is_down = p1.y >= p0.y;
    let xy0 = select(p1, p0, is_down);
    let xy1 = select(p0, p1, is_down);

    let s0 = vec2f(xy0.x * TILE_SCALE, xy0.y * TILE_SCALE);
    let s1 = vec2f(xy1.x * TILE_SCALE, xy1.y * TILE_SCALE);
    let count_x = span(s0.x, s1.x) - 1u;
    let count = count_x + span(s0.y, s1.y);

    let dx = abs(s1.x - s0.x);
    let dy = s1.y - s0.y;
    if dx + dy == 0.0 {
        return invalid;
    }
    if dy == 0.0 && floor(s0.y) == s0.y {
        return invalid;
    }

    let idxdy = 1.0 / (dx + dy);
    var a = dx * idxdy;
    let is_positive_slope = s1.x >= s0.x;
    let line_sign = select(-1.0, 1.0, is_positive_slope);
    let xt0 = floor(s0.x * line_sign);
    let c = s0.x * line_sign - xt0;
    let y0 = floor(s0.y);
    let ytop = select(y0 + 1.0, ceil(s0.y), s0.y == s1.y);
    let b = min((dy * c + dx * (ytop - s0.y)) * idxdy, 0.99999994);
    let robust_err = floor(a * (f32(count) - 1.0) + b) - f32(count_x);
    if robust_err != 0.0 {
        a -= 2e-7 * sign(robust_err);
    }
    let x0 = xt0 * line_sign + select(-1.0, 0.0, is_positive_slope);

    let xmin = min(s0.x, s1.x);
    if s0.y >= f32(bbox.y1) || s1.y < f32(bbox.y0) || xmin >= f32(bbox.x1) {
        return invalid;
    }

    var imin = 0u;
    if s0.y < f32(bbox.y0) {
        var iminf = round((f32(bbox.y0) - y0 + b - a) / (1.0 - a)) - 1.0;
        if y0 + iminf - floor(a * iminf + b) < f32(bbox.y0) {
            iminf += 1.0;
        }
        imin = u32(iminf);
    }
    var imax = count;
    if s1.y > f32(bbox.y1) {
        var imaxf = round((f32(bbox.y1) - y0 + b - a) / (1.0 - a)) - 1.0;
        if y0 + imaxf - floor(a * imaxf + b) < f32(bbox.y1) {
            imaxf += 1.0;
        }
        imax = u32(imaxf);
    }

    let delta = select(1, -1, is_down);
    var ymin = 0;
    var ymax = 0;
    if max(s0.x, s1.x) <= f32(bbox.x0) {
        ymin = i32(ceil(s0.y));
        ymax = i32(ceil(s1.y));
        imax = imin;
    } else {
        let fudge = select(1.0, 0.0, is_positive_slope);
        if xmin < f32(bbox.x0) {
            var f = round((line_sign * (f32(bbox.x0) - x0) - b + fudge) / a);
            if (x0 + line_sign * floor(a * f + b) < f32(bbox.x0)) == is_positive_slope {
                f += 1.0;
            }
            let ynext = i32(y0 + f - floor(a * f + b) + 1.0);
            if is_positive_slope {
                if u32(f) > imin {
                    ymin = i32(y0 + select(1.0, 0.0, y0 == s0.y));
                    ymax = ynext;
                    imin = u32(f);
                }
            } else if u32(f) < imax {
                ymin = ynext;
                ymax = i32(ceil(s1.y));
                imax = u32(f);
            }
        }
        if max(s0.x, s1.x) > f32(bbox.x1) {
            var f = round((line_sign * (f32(bbox.x1) - x0) - b + fudge) / a);
            if (x0 + line_sign * floor(a * f + b) < f32(bbox.x1)) == is_positive_slope {
                f += 1.0;
            }
            if is_positive_slope {
                imax = min(imax, u32(f));
            } else {
                imin = max(imin, u32(f));
            }
        }
    }
    imax = max(imin, imax);
    ymin = max(ymin, bbox.y0);
    ymax = min(ymax, bbox.y1);

    return ScanLinePlan(
        xy0, xy1, is_down, is_positive_slope,
        a, b, x0, line_sign, y0, delta,
        imin, imax, ymin, ymax, true,
    );
}

fn count_tile_hits(
    plan: ScanLinePlan,
    bbox: TileBbox,
    stride: u32,
    starts_base: u32,
) {
    let stride_i = i32(stride);
    for (var i = plan.imin; i < plan.imax; i++) {
        let z = floor(plan.a * f32(i) + plan.b);
        let y = i32(plan.y0 + f32(i) - z);
        let x = i32(plan.x0 + plan.line_sign * z);
        if y < bbox.y0 || y >= bbox.y1 || x < bbox.x0 || x >= bbox.x1 {
            continue;
        }
        let local_ix = u32((y - bbox.y0) * stride_i + x - bbox.x0);
        atomicAdd(&starts[starts_base + local_ix], 1u);
    }
}

fn clip_line_to_tile(
    xy0: vec2f,
    xy1: vec2f,
    is_down: bool,
    is_positive_slope: bool,
    tile_x: i32,
    tile_y: i32,
    seg_within_line: u32,
    seg_count: u32,
    a: f32,
    b: f32,
) -> TileSegment {
    var p0 = xy0;
    var p1 = xy1;
    let tile_xy = vec2f(f32(tile_x) * TILE_SIZE, f32(tile_y) * TILE_SIZE);
    let tile_xy1 = tile_xy + vec2f(TILE_SIZE, TILE_SIZE);

    if seg_within_line > 0u {
        let z_prev = floor(a * (f32(seg_within_line) - 1.0) + b);
        let z = floor(a * f32(seg_within_line) + b);
        if z == z_prev {
            var xt = p0.x + (p1.x - p0.x) * (tile_xy.y - p0.y) / (p1.y - p0.y);
            xt = clamp(xt, tile_xy.x + 1e-3, tile_xy1.x);
            p0 = vec2f(xt, tile_xy.y);
        } else {
            let x_clip = select(tile_xy1.x, tile_xy.x, is_positive_slope);
            var yt = p0.y + (p1.y - p0.y) * (x_clip - p0.x) / (p1.x - p0.x);
            yt = clamp(yt, tile_xy.y + 1e-3, tile_xy1.y);
            p0 = vec2f(x_clip, yt);
        }
    }
    if seg_within_line < seg_count - 1u {
        let z_next = floor(a * (f32(seg_within_line) + 1.0) + b);
        let z = floor(a * f32(seg_within_line) + b);
        if z == z_next {
            var xt = p0.x + (p1.x - p0.x) * (tile_xy1.y - p0.y) / (p1.y - p0.y);
            xt = clamp(xt, tile_xy.x + 1e-3, tile_xy1.x);
            p1 = vec2f(xt, tile_xy1.y);
        } else {
            let x_clip = select(tile_xy.x, tile_xy1.x, is_positive_slope);
            var yt = p0.y + (p1.y - p0.y) * (x_clip - p0.x) / (p1.x - p0.x);
            yt = clamp(yt, tile_xy.y + 1e-3, tile_xy1.y);
            p1 = vec2f(x_clip, yt);
        }
    }

    var y_edge = 1e9;
    var lp0 = p0 - tile_xy;
    var lp1 = p1 - tile_xy;
    const EPSILON: f32 = 1e-6;

    if lp0.x == 0.0 {
        if lp1.x == 0.0 {
            lp0.x = EPSILON;
            if lp0.y == 0.0 {
                lp1.x = EPSILON;
                lp1.y = TILE_SIZE;
            } else {
                lp1.x = 2.0 * EPSILON;
                lp1.y = lp0.y;
            }
        } else if lp0.y == 0.0 {
            lp0.x = EPSILON;
        } else {
            y_edge = lp0.y;
        }
    } else if lp1.x == 0.0 {
        if lp1.y == 0.0 {
            lp1.x = EPSILON;
        } else {
            y_edge = lp1.y;
        }
    }
    if lp0.x == floor(lp0.x) && lp0.x != 0.0 {
        lp0.x -= EPSILON;
    }
    if lp1.x == floor(lp1.x) && lp1.x != 0.0 {
        lp1.x -= EPSILON;
    }
    if !is_down {
        let tmp = lp0;
        lp0 = lp1;
        lp1 = tmp;
    }

    return TileSegment(0u, 0u, y_edge, lp0.x, lp0.y, lp1.x, lp1.y);
}

fn apply_plan_atomic(
    plan: ScanLinePlan,
    bbox: TileBbox,
    stride: u32,
    backdrop_base: u32,
    width_in_tiles: u32,
    path_ix: u32,
) {
    let stride_i = i32(stride);

    for (var y = plan.ymin; y < plan.ymax; y++) {
        let base = u32((y - bbox.y0) * stride_i);
        atomicAdd(&backdrop_pool[backdrop_base + base], plan.delta);
    }

    var last_z = floor(plan.a * (f32(plan.imin) - 1.0) + plan.b);
    for (var i = plan.imin; i < plan.imax; i++) {
        let z = floor(plan.a * f32(i) + plan.b);
        let y = i32(plan.y0 + f32(i) - z);
        let x = i32(plan.x0 + plan.line_sign * z);
        if y < bbox.y0 || y >= bbox.y1 || x < bbox.x0 || x >= bbox.x1 {
            last_z = z;
            continue;
        }
        let starts_on_tile_edge = abs(plan.y0 - plan.xy0.y * TILE_SCALE) <= 1.0e-5;
        let top_edge = select(last_z == z, starts_on_tile_edge, i == plan.imin);
        if top_edge && x + 1 < bbox.x1 {
            let x_bump = max(x + 1, bbox.x0);
            let bump_ix = u32((y - bbox.y0) * stride_i + x_bump - bbox.x0);
            atomicAdd(&backdrop_pool[backdrop_base + bump_ix], plan.delta);
        }

        var seg = clip_line_to_tile(
            plan.xy0, plan.xy1, plan.is_down, plan.is_positive_slope,
            x, y, i - plan.imin, plan.imax - plan.imin, plan.a, plan.b,
        );
        seg.path_ix = path_ix;
        seg.tile_ix = u32(y) * width_in_tiles + u32(x);
        let local_ix = u32((y - bbox.y0) * stride_i + x - bbox.x0);
        let output_ix = atomicAdd(&cursors[backdrop_base + local_ix], 1u);
        segments[output_ix] = seg;
        last_z = z;
    }
}

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let path_id = workgroup_id.x;
    if path_id >= params.path_count {
        return;
    }

    let rec = paths[path_id];
    var bd = backdrops[path_id];
    let bbox = tile_bbox(bd);
    let stride = bd.tile_x1 - bd.tile_x0;
    let backdrop_len = stride * (bd.tile_y1 - bd.tile_y0);
    let backdrop_base = bd.data_offset;
    let starts_base = bd.data_offset + bd.path_ix;
    let seg_start = bd.segment_start;
    let line_start = rec.line_start;
    let line_count = rec.line_count;

    for (var i = local_id.x; i < backdrop_len; i += WORKGROUP_SIZE) {
        atomicStore(&backdrop_pool[backdrop_base + i], 0);
        atomicStore(&starts[starts_base + i], 0u);
        atomicStore(&cursors[backdrop_base + i], 0u);
    }
    if local_id.x == 0u {
        atomicStore(&starts[starts_base + backdrop_len], 0u);
    }
    workgroupBarrier();

    for (var line_i = local_id.x; line_i < line_count; line_i += WORKGROUP_SIZE) {
        let line = lines[line_start + line_i];
        let plan = plan_scan_line(line, bbox);
        if plan.valid {
            count_tile_hits(plan, bbox, stride, starts_base);
        }
    }
    workgroupBarrier();

    if local_id.x == 0u {
        var out_ix = seg_start;
        for (var i = 0u; i < backdrop_len; i++) {
            let count = atomicLoad(&starts[starts_base + i]);
            atomicStore(&starts[starts_base + i], out_ix);
            atomicStore(&cursors[backdrop_base + i], out_ix);
            out_ix += count;
        }
        atomicStore(&starts[starts_base + backdrop_len], out_ix);
        bd.segment_count = out_ix - seg_start;
        backdrops[path_id] = bd;
    }
    workgroupBarrier();

    for (var line_i = local_id.x; line_i < line_count; line_i += WORKGROUP_SIZE) {
        let line = lines[line_start + line_i];
        let plan = plan_scan_line(line, bbox);
        if !plan.valid {
            continue;
        }
        apply_plan_atomic(plan, bbox, stride, backdrop_base, params.width_in_tiles, bd.path_ix);
    }
}
