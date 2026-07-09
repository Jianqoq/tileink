#include "common.wgsl"

@group(0) @binding(1) var<storage, read> path_records: array<PathRecord>;
@group(0) @binding(2) var<storage, read> scan_chunk_ranges: array<GpuScanChunkRange>;
@group(0) @binding(3) var<storage, read_write> segment_bumps: array<u32>;
@group(0) @binding(4) var<storage, read_write> chunk_totals: array<u32>;
@group(0) @binding(5) var<storage, read_write> chunk_offsets: array<u32>;

@compute @workgroup_size(256)
fn scan_chunk_offsets(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let path_id = global_id.x;
    if (path_id >= config.path_count) {
        return;
    }
    let path = path_records[path_id];
    var next = path.segment_start;
    let chunk_range = scan_chunk_ranges[path_id];
    var chunk_ix = chunk_range.start;
    let chunk_end = chunk_range.end;
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

