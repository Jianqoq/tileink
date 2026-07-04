#include "common.wgsl"

@group(0) @binding(3) var<storage, read> scan_chunk_backdrop_offsets: array<u32>;
@group(0) @binding(4) var<storage, read> scan_chunk_lens: array<u32>;
@group(0) @binding(8) var<storage, read_write> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(10) var<storage, read_write> segment_tile_counts: array<atomic<u32>>;
@group(0) @binding(13) var<storage, read_write> chunk_totals: array<u32>;

var<workgroup> scan_scratch: array<u32, 256>;

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

