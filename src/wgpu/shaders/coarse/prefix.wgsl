#include "common.wgsl"

@group(0) @binding(25) var<storage, read_write> tile_records: array<TileCoarseRecord>;
@group(0) @binding(31) var<storage, read_write> chunk_records: array<CoarseChunkRecord>;

@compute @workgroup_size(256)
fn coarse_ptcl_prefix_chunks(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    prefix_chunks(workgroup_id.x, local_id.x, false);
}

@compute @workgroup_size(256)
fn coarse_glyph_prefix_chunks(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    prefix_chunks(workgroup_id.x, local_id.x, true);
}

fn prefix_chunks(chunk_ix: u32, lane: u32, glyph: bool) {
    let chunk_offset = chunk_ix * 256u;
    let chunk_len = min(config.tile_count - chunk_offset, 256u);
    var count = 0u;
    if (lane < chunk_len) {
        let tile_ix = chunk_offset + lane;
        if (glyph) {
            count = tile_records[tile_ix].glyph_count;
        } else {
            count = tile_records[tile_ix].ptcl_count;
        }
    }
    coarse_scratch[lane] = count;
    workgroupBarrier();

    var step = 1u;
    loop {
        if (step >= 256u) {
            break;
        }
        let ix = (lane + 1u) * step * 2u - 1u;
        if (ix < 256u) {
            coarse_scratch[ix] += coarse_scratch[ix - step];
        }
        workgroupBarrier();
        step *= 2u;
    }

    if (lane == 0u) {
        if (glyph) {
            chunk_records[chunk_ix].glyph_total = coarse_scratch[255u];
        } else {
            chunk_records[chunk_ix].ptcl_total = coarse_scratch[255u];
        }
        coarse_scratch[255u] = 0u;
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
            let previous_left = coarse_scratch[left];
            coarse_scratch[left] = coarse_scratch[ix];
            coarse_scratch[ix] += previous_left;
        }
        workgroupBarrier();
        step /= 2u;
    }

    if (lane < chunk_len) {
        let tile_ix = chunk_offset + lane;
        let start = coarse_scratch[lane];
        if (glyph) {
            tile_records[tile_ix].glyph_start = start;
            tile_records[tile_ix].glyph_end = start + count;
        } else {
            tile_records[tile_ix].ptcl_start = start;
            tile_records[tile_ix].ptcl_end = start + count;
        }
    }
}

@compute @workgroup_size(1)
fn coarse_ptcl_chunk_offsets() {
    var carry = 0u;
    var chunk_ix = 0u;
    loop {
        if (chunk_ix >= config.chunk_count) {
            break;
        }
        chunk_records[chunk_ix].ptcl_offset = carry;
        carry += chunk_records[chunk_ix].ptcl_total;
        chunk_ix += 1u;
    }
}

@compute @workgroup_size(1)
fn coarse_glyph_chunk_offsets() {
    var carry = 0u;
    var chunk_ix = 0u;
    loop {
        if (chunk_ix >= config.chunk_count) {
            break;
        }
        chunk_records[chunk_ix].glyph_offset = carry;
        carry += chunk_records[chunk_ix].glyph_total;
        chunk_ix += 1u;
    }
}

@compute @workgroup_size(256)
fn coarse_ptcl_apply_chunk_offsets(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    apply_chunk_offsets(workgroup_id.x, local_id.x, false);
}

@compute @workgroup_size(256)
fn coarse_glyph_apply_chunk_offsets(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    apply_chunk_offsets(workgroup_id.x, local_id.x, true);
}

fn apply_chunk_offsets(chunk_ix: u32, lane: u32, glyph: bool) {
    let tile_ix = chunk_ix * 256u + lane;
    if (tile_ix >= config.tile_count) {
        return;
    }
    if (glyph) {
        let offset = chunk_records[chunk_ix].glyph_offset;
        tile_records[tile_ix].glyph_start += offset;
        tile_records[tile_ix].glyph_end += offset;
    } else {
        let offset = chunk_records[chunk_ix].ptcl_offset;
        tile_records[tile_ix].ptcl_start += offset;
        tile_records[tile_ix].ptcl_end += offset;
    }
}
