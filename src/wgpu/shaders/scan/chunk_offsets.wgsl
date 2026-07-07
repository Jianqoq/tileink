#include "common.wgsl"

@group(0) @binding(1) var<storage, read> path_records: array<PathRecord>;
@group(0) @binding(2) var<storage, read> scan_chunk_range_starts: array<u32>;
@group(0) @binding(3) var<storage, read> scan_chunk_range_ends: array<u32>;
@group(0) @binding(4) var<storage, read_write> segment_bumps: array<u32>;
@group(0) @binding(5) var<storage, read_write> chunk_totals: array<u32>;
@group(0) @binding(6) var<storage, read_write> chunk_offsets: array<u32>;

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

