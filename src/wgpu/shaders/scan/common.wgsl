struct ScanConfig {
    clear_len: u32,
    backdrop_len: u32,
    path_count: u32,
    scan_chunk_count: u32,
    line_count: u32,
    segment_capacity: u32,
    incremental: u32,
    line_base: u32,
    path_base: u32,
    chunk_base: u32,
    backdrop_base: u32,
};

struct Line {
    path_id: u32,
    _pad: f32,
    p0: vec2<f32>,
    p1: vec2<f32>,
};

struct AffineRecord {
    a: f32, b: f32, c: f32, d: f32, e: f32, f: f32,
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
    transform: AffineRecord,
};

fn affine_point(transform: AffineRecord, point: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        transform.a * point.x + transform.c * point.y + transform.e,
        transform.b * point.x + transform.d * point.y + transform.f,
    );
}

struct GpuScanChunk {
    path_id: u32,
    backdrop_offset: u32,
    segment_start: u32,
    len: u32,
};

struct GpuScanChunkRange {
    start: u32,
    end: u32,
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

fn linear_workgroup_index(workgroup_id: vec3<u32>, num_workgroups: vec3<u32>) -> u32 {
    return workgroup_id.x + workgroup_id.y * num_workgroups.x +
        workgroup_id.z * num_workgroups.x * num_workgroups.y;
}

fn dispatched_index(local_ix: u32, base: u32) -> u32 {
    if (config.incremental != 0u) {
        return active_indices[base + local_ix];
    }
    return local_ix;
}

// Shared by DDA top-edge detection, top-clipped backdrop bumps, and tile-boundary
// segment snapping. This only absorbs arithmetic noise around an exact tile
// boundary; wider tolerances can create false backdrop carry for nearby geometry.
const SCAN_EPSILON: f32 = 1.0e-6;
const TOP_TOUCH_EPSILON: f32 = 1.0e-12;

// DDA-derived top/bottom clips are nudged into the tile before y_edge handling.
// This is not a comparison tolerance: at global pixel coordinates, a 1e-6 offset can
// round back to the boundary in f32 and be misclassified as a left-edge crossing.
const TILE_CLIP_NUDGE: f32 = 1.0e-3;

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

fn is_tile_boundary_y(value: f32) -> bool {
    return abs(value - floor(value)) <= TOP_TOUCH_EPSILON;
}

fn ceil_tile_boundary_y(value: f32) -> i32 {
    if (is_tile_boundary_y(value)) {
        return i32(floor(value));
    }
    return i32(ceil(value));
}

fn local_tile_ix(tile_x: i32, tile_y: i32, bbox_x0: u32, bbox_y0: u32, bbox_x1: u32) -> u32 {
    let local_x = u32(tile_x) - bbox_x0;
    let local_y = u32(tile_y) - bbox_y0;
    return local_y * (bbox_x1 - bbox_x0) + local_x;
}
