// Scan/cumsum dispatches are ordered; disjoint row chunks give each backdrop one writer.
struct CumsumConfig {
    row_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> config: CumsumConfig;
@group(0) @binding(1) var<storage, read> chunk_backdrop_offsets: array<u32>;
@group(0) @binding(2) var<storage, read> chunk_lens: array<u32>;
@group(0) @binding(3) var<storage, read> row_chunk_starts: array<u32>;
@group(0) @binding(4) var<storage, read> row_chunk_ends: array<u32>;
@group(0) @binding(5) var<storage, read_write> backdrops: array<i32>;
@group(0) @binding(6) var<storage, read_write> chunk_totals: array<i32>;
@group(0) @binding(7) var<storage, read_write> chunk_offsets: array<i32>;

var<workgroup> scratch: array<i32, 256>;

fn linear_workgroup_index(workgroup_id: vec3<u32>, num_workgroups: vec3<u32>) -> u32 {
    return workgroup_id.x + workgroup_id.y * num_workgroups.x +
        workgroup_id.z * num_workgroups.x * num_workgroups.y;
}

@compute @workgroup_size(256)
fn cumsum_prefix_chunks(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let chunk_ix = linear_workgroup_index(workgroup_id, num_workgroups);
    let lane = local_id.x;
    let chunk_offset = chunk_backdrop_offsets[chunk_ix];
    let chunk_len = chunk_lens[chunk_ix];

    var value = 0i;
    if (lane < chunk_len) {
        value = backdrops[chunk_offset + lane];
    }
    scratch[lane] = value;
    workgroupBarrier();

    var step = 1u;
    loop {
        if (step >= 256u) {
            break;
        }
        let ix = (lane + 1u) * step * 2u - 1u;
        if (ix < 256u) {
            scratch[ix] += scratch[ix - step];
        }
        workgroupBarrier();
        step *= 2u;
    }

    if (lane == 0u) {
        chunk_totals[chunk_ix] = scratch[255u];
        scratch[255u] = 0i;
    }
    workgroupBarrier();

    step = 128u;
    loop {
        if (step == 0u) {
            break;
        }
        let ix = (lane + 1u) * step * 2u - 1u;
        if (ix < 256u) {
            let left = ix - step;
            let previous_left = scratch[left];
            scratch[left] = scratch[ix];
            scratch[ix] += previous_left;
        }
        workgroupBarrier();
        step /= 2u;
    }

    if (lane < chunk_len) {
        backdrops[chunk_offset + lane] = scratch[lane] + value;
    }
}

@compute @workgroup_size(256)
fn cumsum_chunk_offsets(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let row_ix = global_id.x;
    if (row_ix >= config.row_count) {
        return;
    }

    var carry = 0i;
    var chunk_ix = row_chunk_starts[row_ix];
    let chunk_end = row_chunk_ends[row_ix];
    loop {
        if (chunk_ix >= chunk_end) {
            break;
        }
        chunk_offsets[chunk_ix] = carry;
        carry += chunk_totals[chunk_ix];
        chunk_ix += 1u;
    }
}

@compute @workgroup_size(256)
fn cumsum_apply_chunk_offsets(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let chunk_ix = linear_workgroup_index(workgroup_id, num_workgroups);
    let lane = local_id.x;
    let chunk_len = chunk_lens[chunk_ix];
    if (lane >= chunk_len) {
        return;
    }

    let ix = chunk_backdrop_offsets[chunk_ix] + lane;
    backdrops[ix] += chunk_offsets[chunk_ix];
}
