struct CoarseConfig {
    tile_count: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_start: u32,
    draw_end: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    ptcl_capacity: u32,
    glyph_capacity: u32,
    chunk_count: u32,
    text_run_count: u32,
    text_glyph_count: u32,
    tile_draw_index_count: u32,
    emit_chunk_capacity: u32,
};

@group(0) @binding(0) var<uniform> config: CoarseConfig;

struct DrawRecord {
    path_id: u32,
    glyph_run_id: u32,
    sdf_offset: u32,
    sdf_len: u32,
    sdf_shadow_offset: u32,
    sdf_shadow_len: u32,
    brush_offset: u32,
    brush_len: u32,
    tag: u32,
    fill_rule: u32,
    pixel_x0: i32,
    pixel_y0: i32,
    pixel_x1: i32,
    pixel_y1: i32,
    solid_rect: u32,
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
struct GlyphRunRecord {
    glyph_start: u32,
    glyph_count: u32,
};
struct GlyphRecord {
    image_id: u32,
    x: i32,
    y: i32,
};
struct GlyphImageRecord {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    content: u32,
    data_offset: u32,
};
struct TileCoarseRecord {
    ptcl_count: u32,
    ptcl_start: u32,
    ptcl_end: u32,
    glyph_count: u32,
    glyph_start: u32,
    glyph_end: u32,
};
struct CoarseChunkRecord {
    ptcl_total: u32,
    ptcl_offset: u32,
    glyph_total: u32,
    glyph_offset: u32,
};
struct TileEmitChunkRecord {
    count: u32,
    offset: u32,
};
struct EmitChunkRecord {
    tile: u32,
    local_chunk: u32,
    ptcl_count: u32,
    ptcl_offset: u32,
    glyph_count: u32,
    glyph_offset: u32,
};
struct LayerStackRecord {
    tag: u32,
    draw: u32,
    payload: u32,
};
struct PtclRecord {
    tag: u32,
    backdrop: i32,
    fill_rule: u32,
    segment_start: u32,
    segment_end: u32,
    color: u32,
};
struct TileSegmentRange {
    start: u32,
    end: u32,
};

const INVALID: u32 = 0xffffffffu;
const GPU_DRAW_BRUSH: u32 = 0u;
const GPU_DRAW_CLIP: u32 = 1u;
const GPU_DRAW_OPACITY: u32 = 2u;
const GPU_DRAW_BLEND: u32 = 3u;
const GPU_DRAW_ISOLATE: u32 = 4u;
const GPU_DRAW_PATH_GLYPH: u32 = 5u;
const GPU_BRUSH_SOLID: u32 = 1u;
const GPU_LAYER_CLIP: u32 = 0u;
const GPU_LAYER_OPACITY: u32 = 1u;
const GPU_LAYER_BLEND: u32 = 2u;
const GPU_PTCL_END: u32 = 0u;
const GPU_PTCL_FILL: u32 = 1u;
const GPU_PTCL_COLOR: u32 = 2u;
const GPU_PTCL_BEGIN_CLIP: u32 = 3u;
const GPU_PTCL_END_CLIP: u32 = 4u;
const GPU_PTCL_BEGIN_OPACITY: u32 = 5u;
const GPU_PTCL_END_OPACITY: u32 = 6u;
const GPU_PTCL_BEGIN_BLEND: u32 = 7u;
const GPU_PTCL_END_BLEND: u32 = 8u;
const GPU_PTCL_SDF: u32 = 9u;
const GPU_PTCL_GLYPH: u32 = 10u;
const GPU_PTCL_PATH_GLYPH: u32 = 11u;
const GPU_PTCL_BEGIN_SDF_CLIP: u32 = 12u;
const GPU_SDF_RECT: u32 = 1u;
const FULL_TILE_SDF_SOLID_DISTANCE: f32 = -0.75;
const GLYPH_RUN_RECORD_WORDS: u32 = 2u;
const GLYPH_RECORD_WORDS: u32 = 3u;
const GLYPH_IMAGE_RECORD_WORDS: u32 = 6u;
const TILE_COARSE_RECORD_WORDS: u32 = 6u;
const PTCL_RECORD_WORDS: u32 = 6u;
const TILE_DRAW_RECORD_WORDS: u32 = 2u;
const TILE_EMIT_CHUNK_RECORD_WORDS: u32 = 2u;
const EMIT_CHUNK_RECORD_WORDS: u32 = 6u;

fn coarse_tile_base(tile_ix: u32) -> u32 {
    return tile_ix * TILE_COARSE_RECORD_WORDS;
}

fn coarse_ptcl_base(ptcl_ix: u32) -> u32 {
    return config.tile_count * TILE_COARSE_RECORD_WORDS + ptcl_ix * PTCL_RECORD_WORDS;
}

fn coarse_glyph_base(glyph_ix: u32) -> u32 {
    return config.tile_count * TILE_COARSE_RECORD_WORDS +
        config.ptcl_capacity * PTCL_RECORD_WORDS +
        glyph_ix;
}

fn coarse_tile_draw_record_base(tile_ix: u32) -> u32 {
    return coarse_glyph_base(config.glyph_capacity) + tile_ix * TILE_DRAW_RECORD_WORDS;
}

fn coarse_tile_draw_index_base() -> u32 {
    return coarse_tile_draw_record_base(config.tile_count);
}

fn coarse_tile_emit_chunk_record_base() -> u32 {
    return coarse_tile_draw_index_base() + config.tile_draw_index_count;
}

fn coarse_emit_chunk_record_base() -> u32 {
    return coarse_tile_emit_chunk_record_base() + config.tile_count * TILE_EMIT_CHUNK_RECORD_WORDS;
}

fn tile_draw_start_at(tile_ix: u32) -> u32 {
    return coarse_work[coarse_tile_draw_record_base(tile_ix)];
}

fn tile_draw_end_at(tile_ix: u32) -> u32 {
    return coarse_work[coarse_tile_draw_record_base(tile_ix) + 1u];
}

fn tile_draw_index_at(draw_ref_ix: u32) -> u32 {
    return coarse_work[coarse_tile_draw_index_base() + draw_ref_ix];
}

fn tile_emit_chunk_record_base(tile_ix: u32) -> u32 {
    return coarse_tile_emit_chunk_record_base() + tile_ix * TILE_EMIT_CHUNK_RECORD_WORDS;
}

fn tile_emit_chunk_count_at(tile_ix: u32) -> u32 {
    return coarse_work[tile_emit_chunk_record_base(tile_ix)];
}

fn tile_emit_chunk_offset_at(tile_ix: u32) -> u32 {
    return coarse_work[tile_emit_chunk_record_base(tile_ix) + 1u];
}

fn store_tile_emit_chunk_count(tile_ix: u32, count: u32) {
    coarse_work[tile_emit_chunk_record_base(tile_ix)] = count;
}

fn store_tile_emit_chunk_offset(tile_ix: u32, offset: u32) {
    coarse_work[tile_emit_chunk_record_base(tile_ix) + 1u] = offset;
}

fn emit_chunk_record_base(ref_ix: u32) -> u32 {
    return coarse_emit_chunk_record_base() + ref_ix * EMIT_CHUNK_RECORD_WORDS;
}

fn emit_chunk_at(ref_ix: u32) -> EmitChunkRecord {
    let base = emit_chunk_record_base(ref_ix);
    return EmitChunkRecord(
        coarse_work[base],
        coarse_work[base + 1u],
        coarse_work[base + 2u],
        coarse_work[base + 3u],
        coarse_work[base + 4u],
        coarse_work[base + 5u],
    );
}

fn store_emit_chunk_ref(ref_ix: u32, tile_ix: u32, local_chunk: u32) {
    let base = emit_chunk_record_base(ref_ix);
    coarse_work[base] = tile_ix;
    coarse_work[base + 1u] = local_chunk;
}

fn store_emit_chunk_counts(ref_ix: u32, ptcl_count: u32, glyph_count: u32) {
    let base = emit_chunk_record_base(ref_ix);
    coarse_work[base + 2u] = ptcl_count;
    coarse_work[base + 4u] = glyph_count;
}

fn store_emit_chunk_offsets(ref_ix: u32, ptcl_offset: u32, glyph_offset: u32) {
    let base = emit_chunk_record_base(ref_ix);
    coarse_work[base + 3u] = ptcl_offset;
    coarse_work[base + 5u] = glyph_offset;
}

fn coarse_load_tile(tile_ix: u32) -> TileCoarseRecord {
    let base = coarse_tile_base(tile_ix);
    return TileCoarseRecord(
        coarse_work[base],
        coarse_work[base + 1u],
        coarse_work[base + 2u],
        coarse_work[base + 3u],
        coarse_work[base + 4u],
        coarse_work[base + 5u],
    );
}

fn coarse_store_tile_counts(tile_ix: u32, ptcl_count: u32, glyph_count: u32) {
    let base = coarse_tile_base(tile_ix);
    coarse_work[base] = ptcl_count;
    coarse_work[base + 3u] = glyph_count;
}

fn coarse_tile_count(tile_ix: u32, glyph: bool) -> u32 {
    let base = coarse_tile_base(tile_ix);
    if (glyph) {
        return coarse_work[base + 3u];
    }
    return coarse_work[base];
}

fn coarse_store_tile_range(tile_ix: u32, glyph: bool, start: u32, end: u32) {
    let base = coarse_tile_base(tile_ix);
    if (glyph) {
        coarse_work[base + 4u] = start;
        coarse_work[base + 5u] = end;
    } else {
        coarse_work[base + 1u] = start;
        coarse_work[base + 2u] = end;
    }
}

fn coarse_add_tile_range_offset(tile_ix: u32, glyph: bool, offset: u32) {
    let base = coarse_tile_base(tile_ix);
    if (glyph) {
        coarse_work[base + 4u] += offset;
        coarse_work[base + 5u] += offset;
    } else {
        coarse_work[base + 1u] += offset;
        coarse_work[base + 2u] += offset;
    }
}

fn coarse_store_ptcl(
    ptcl_ix: u32,
    tag: u32,
    backdrop: i32,
    fill_rule: u32,
    segment_start: u32,
    segment_end: u32,
    color: u32,
) {
    let base = coarse_ptcl_base(ptcl_ix);
    coarse_work[base] = tag;
    coarse_work[base + 1u] = bitcast<u32>(backdrop);
    coarse_work[base + 2u] = fill_rule;
    coarse_work[base + 3u] = segment_start;
    coarse_work[base + 4u] = segment_end;
    coarse_work[base + 5u] = color;
}

fn coarse_store_glyph(glyph_ix: u32, source_glyph_ix: u32) {
    coarse_work[coarse_glyph_base(glyph_ix)] = source_glyph_ix;
}

var<workgroup> coarse_scratch: array<u32, 256>;
var<workgroup> coarse_total: u32;

fn workgroup_sum(value: u32, lane: u32) -> u32 {
    _ = workgroup_exclusive_prefix(value, lane);
    return coarse_total;
}

fn workgroup_exclusive_prefix(value: u32, lane: u32) -> u32 {
    coarse_scratch[lane] = value;
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
        coarse_total = coarse_scratch[255u];
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

    let offset = coarse_scratch[lane];
    workgroupBarrier();
    return offset;
}

fn pixel_tile_min(value: i32, limit: u32) -> u32 {
    var tile = 0u;
    if (value > 0i) {
        tile = min(u32(value) / 16u, limit);
    }
    return tile;
}

fn pixel_tile_max(value: i32, limit: u32) -> u32 {
    var tile = 0u;
    if (value > 0i) {
        tile = min((u32(value) + 15u) / 16u, limit);
    }
    return tile;
}
