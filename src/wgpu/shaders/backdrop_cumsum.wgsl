// Per-path row inclusive prefix sum over disjoint `backdrop_pool[]` slices.
// One workgroup per path; each row uses Blelloch exclusive scan in workgroup memory.

const WORKGROUP_SIZE: u32 = 256u;

struct CumsumParams {
    path_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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

@group(0) @binding(0) var<uniform> params: CumsumParams;
@group(0) @binding(1) var<storage, read_write> backdrops: array<BackdropRecord>;
@group(0) @binding(2) var<storage, read_write> backdrop_pool: array<i32>;

var<workgroup> shared_data: array<i32, WORKGROUP_SIZE>;
var<workgroup> wg_carry: i32;
var<workgroup> wg_block_sum: i32;

fn next_pow2(x: u32) -> u32 {
    if (x <= 1u) {
        return 1u;
    }
    var v = x - 1u;
    v |= v >> 1u;
    v |= v >> 2u;
    v |= v >> 4u;
    v |= v >> 8u;
    v |= v >> 16u;
    return v + 1u;
}

fn blelloch_upsweep(local_id: u32, n: u32) {
    var offset = 1u;
    loop {
        if (offset >= n) {
            break;
        }
        workgroupBarrier();
        let index = (local_id + 1u) * offset * 2u - 1u;
        if (index < n) {
            shared_data[index] += shared_data[index - offset];
        }
        workgroupBarrier();
        offset <<= 1u;
    }
}

fn blelloch_downsweep(local_id: u32, n: u32) {
    var offset = n >> 1u;
    loop {
        if (offset == 0u) {
            break;
        }
        workgroupBarrier();
        let index = (local_id + 1u) * offset * 2u - 1u;
        if (index < n) {
            let t = shared_data[index - offset];
            shared_data[index - offset] = shared_data[index];
            shared_data[index] += t;
        }
        workgroupBarrier();
        offset >>= 1u;
    }
}

fn inclusive_row_scan(base: u32, row_len: u32, local_id: u32) {
    if (row_len <= 1u) {
        return;
    }

    if (local_id == 0u) {
        wg_carry = 0;
    }
    workgroupBarrier();

    var chunk_start = 0u;
    loop {
        if (chunk_start >= row_len) {
            break;
        }
        let chunk_len = min(WORKGROUP_SIZE, row_len - chunk_start);
        let padded = next_pow2(chunk_len);

        if (local_id < padded) {
            if (local_id < chunk_len) {
                shared_data[local_id] = backdrop_pool[base + chunk_start + local_id];
            } else {
                shared_data[local_id] = 0;
            }
        }
        workgroupBarrier();

        blelloch_upsweep(local_id, padded);

        if (local_id == 0u) {
            wg_block_sum = shared_data[padded - 1u];
            shared_data[padded - 1u] = 0;
        }
        workgroupBarrier();

        blelloch_downsweep(local_id, padded);

        if (local_id < chunk_len) {
            let orig = backdrop_pool[base + chunk_start + local_id];
            backdrop_pool[base + chunk_start + local_id] =
                shared_data[local_id] + orig + wg_carry;
        }
        workgroupBarrier();

        if (local_id == 0u) {
            wg_carry += wg_block_sum;
        }
        workgroupBarrier();

        chunk_start += WORKGROUP_SIZE;
    }
}

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id) wg_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let path_id = wg_id.x;
    if (path_id >= params.path_count) {
        return;
    }
    let bd = backdrops[path_id];
    let stride = bd.tile_x1 - bd.tile_x0;
    let height = bd.tile_y1 - bd.tile_y0;
    if (stride == 0u || height == 0u) {
        return;
    }
    let base = bd.data_offset;
    for (var row = 0u; row < height; row++) {
        inclusive_row_scan(base + row * stride, stride, local_id.x);
        workgroupBarrier();
    }
}
