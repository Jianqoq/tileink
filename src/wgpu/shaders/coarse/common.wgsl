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
    paint_brush_base: u32,
    text_enabled: u32,
    active_tile_count: u32,
    active_tile_list_base: u32,
    incremental: u32,
};

@group(0) @binding(0) var<uniform> config: CoarseConfig;

fn linear_workgroup_index(workgroup_id: vec3<u32>, num_workgroups: vec3<u32>) -> u32 {
    return workgroup_id.x + workgroup_id.y * num_workgroups.x +
        workgroup_id.z * num_workgroups.x * num_workgroups.y;
}

fn dispatched_tile_at(dispatch_ix: u32) -> u32 {
    if (config.incremental != 0u) {
        return coarse_work[config.active_tile_list_base + dispatch_ix];
    }
    return dispatch_ix;
}

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
    class_flags: u32,
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
const GPU_BRUSH_PATTERN_RESOURCE: u32 = 7u;
const GPU_LAYER_CLIP: u32 = 0u;
const GPU_LAYER_OPACITY: u32 = 1u;
const GPU_LAYER_BLEND: u32 = 2u;
const GPU_FILL_RULE_EVEN_ODD: u32 = 1u;
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
const GPU_PTCL_IMAGE: u32 = 13u;
const GPU_SDF_RECT: u32 = 1u;
const GPU_SDF_CANDLESTICK: u32 = 5u;
// `sdf_coverage_from_dist` reaches exactly 1.0 at distance -0.5. Using the
// mathematical coverage threshold here lets a sharp rect aligned to a tile
// take the analytic/image fast path without changing any edge pixels.
const FULL_TILE_SDF_SOLID_INSET: f32 = 0.5;
const GLYPH_RUN_RECORD_WORDS: u32 = 2u;
const GLYPH_RECORD_WORDS: u32 = 3u;
const GLYPH_IMAGE_RECORD_WORDS: u32 = 6u;
const TILE_COARSE_RECORD_WORDS: u32 = 6u;
const PTCL_RECORD_WORDS: u32 = 6u;
const TILE_DRAW_RECORD_WORDS: u32 = 2u;
const TILE_EMIT_CHUNK_RECORD_WORDS: u32 = 2u;
const EMIT_CHUNK_RECORD_WORDS: u32 = 7u;
const EMIT_CHUNK_CLASS_COLOR: u32 = 1u;
const EMIT_CHUNK_CLASS_SDF: u32 = 2u;
const EMIT_CHUNK_CLASS_OTHER: u32 = 4u;
const FINE_TILE_KIND_FULL_INTERPRETER: u32 = 0u;
const FINE_TILE_KIND_EMPTY_OR_CLEAR: u32 = 1u;
const FINE_TILE_KIND_COLOR_ONLY_NO_STACK: u32 = 2u;
const FINE_TILE_KIND_PURE_SDF_SOLID_NO_STACK: u32 = 3u;
const FINE_TILE_KIND_MIXED_ANALYTIC_SOLID_NO_STACK: u32 = 4u;
const FINE_TILE_KIND_ANALYTIC_WITH_STACK: u32 = 5u;

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

fn coarse_fine_tile_kind_base() -> u32 {
    return coarse_emit_chunk_record_base() + config.emit_chunk_capacity * EMIT_CHUNK_RECORD_WORDS;
}

fn store_fine_tile_kind(tile_ix: u32, kind: u32) {
    coarse_work[coarse_fine_tile_kind_base() + tile_ix] = kind;
}

const TILE_DRAW_PAGE_SIZE: u32 = 256u;
const TILE_DRAW_PAGE_WORDS: u32 = 257u;

fn tile_draw_head_at(tile_ix: u32) -> u32 {
    return coarse_work[coarse_tile_draw_record_base(tile_ix)];
}

fn tile_draw_count_at(tile_ix: u32) -> u32 {
    return coarse_work[coarse_tile_draw_record_base(tile_ix) + 1u];
}

fn tile_draw_next_page(page: u32) -> u32 {
    return coarse_work[coarse_tile_draw_index_base() + page * TILE_DRAW_PAGE_WORDS];
}

fn tile_draw_index_in_page(page: u32, slot: u32) -> u32 {
    return coarse_work[coarse_tile_draw_index_base() + page * TILE_DRAW_PAGE_WORDS + 1u + slot];
}

fn tile_draw_page_at(tile_ix: u32, local_page: u32) -> u32 {
    var page = tile_draw_head_at(tile_ix);
    var index = 0u;
    loop {
        if (page == INVALID || index >= local_page) {
            return page;
        }
        page = tile_draw_next_page(page);
        index += 1u;
    }
    return INVALID;
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

fn path_backdrop_fully_covers_tile(backdrop_ix: u32, fill_rule: u32) -> bool {
    let segment_range = segment_ranges[backdrop_ix];
    return segment_range.start == segment_range.end &&
        backdrop_value_is_full_alpha(atomicLoad(&backdrops[backdrop_ix]), fill_rule);
}

fn backdrop_value_is_full_alpha(backdrop: i32, fill_rule: u32) -> bool {
    if (fill_rule == GPU_FILL_RULE_EVEN_ODD) {
        return (u32(abs(backdrop)) & 1u) == 1u;
    }
    return backdrop != 0i;
}

fn draw_sdf_clip_fully_covers_tile_at(draw_ix: u32, tile_x: u32, tile_y: u32) -> bool {
    let draw = draw_records[draw_ix];
    return draw.sdf_offset != INVALID &&
        draw.sdf_shadow_offset == INVALID &&
        draw.sdf_len >= 9u &&
        sdf_blob[draw.sdf_offset] == GPU_SDF_RECT &&
        sdf_rect_fully_covers_tile(draw.sdf_offset, tile_x, tile_y);
}

fn sdf_rect_fully_covers_tile(sdf_base: u32, tile_x: u32, tile_y: u32) -> bool {
    // Conservative full-coverage test: pixel centers must stay inside the rect eroded by the
    // coverage ramp, and rounded corners use squared distances so coarse avoids sqrt work.
    let rect_min = vec2<f32>(
        min(sdf_float_at(sdf_base, 1u), sdf_float_at(sdf_base, 3u)),
        min(sdf_float_at(sdf_base, 2u), sdf_float_at(sdf_base, 4u)),
    );
    let rect_max = vec2<f32>(
        max(sdf_float_at(sdf_base, 1u), sdf_float_at(sdf_base, 3u)),
        max(sdf_float_at(sdf_base, 2u), sdf_float_at(sdf_base, 4u)),
    );
    let tile_min = vec2<f32>(f32(tile_x * 16u), f32(tile_y * 16u)) + vec2<f32>(0.5);
    let tile_max = tile_min + vec2<f32>(15.0);
    let rect_size = rect_max - rect_min;
    let min_size = vec2<f32>(15.0 + 2.0 * FULL_TILE_SDF_SOLID_INSET);
    let inset = vec2<f32>(FULL_TILE_SDF_SOLID_INSET);
    var covers =
        all(rect_size >= min_size) &&
        all(tile_min >= rect_min + inset) &&
        all(tile_max <= rect_max - inset);
    if (covers) {
        let radius_limit = min(rect_size.x, rect_size.y) * 0.5;
        let radii = min(max(vec4<f32>(
            sdf_float_at(sdf_base, 5u),
            sdf_float_at(sdf_base, 6u),
            sdf_float_at(sdf_base, 7u),
            sdf_float_at(sdf_base, 8u),
        ), vec4<f32>(0.0)), vec4<f32>(radius_limit));
        if (any(radii > vec4<f32>(0.0))) {
            covers = rounded_rect_corners_fully_cover_tile(rect_min, rect_max, tile_min, tile_max, radii);
        }
    }
    return covers;
}

fn sdf_float_at(sdf_base: u32, index: u32) -> f32 {
    return bitcast<f32>(sdf_blob[sdf_base + index]);
}

fn rounded_rect_corners_fully_cover_tile(
    rect_min: vec2<f32>,
    rect_max: vec2<f32>,
    tile_min: vec2<f32>,
    tile_max: vec2<f32>,
    radii: vec4<f32>,
) -> bool {
    let top_left = rounded_rect_corner_fully_covers(
        tile_min,
        rect_min,
        rect_min + vec2<f32>(radii.x),
        radii.x,
    );
    let top_right = rounded_rect_corner_fully_covers(
        vec2<f32>(tile_max.x, tile_min.y),
        vec2<f32>(rect_max.x, rect_min.y),
        vec2<f32>(rect_max.x - radii.y, rect_min.y + radii.y),
        radii.y,
    );
    let bottom_left = rounded_rect_corner_fully_covers(
        vec2<f32>(tile_min.x, tile_max.y),
        vec2<f32>(rect_min.x, rect_max.y),
        vec2<f32>(rect_min.x + radii.z, rect_max.y - radii.z),
        radii.z,
    );
    let bottom_right = rounded_rect_corner_fully_covers(
        tile_max,
        rect_max,
        rect_max - vec2<f32>(radii.w),
        radii.w,
    );
    return top_left && top_right && bottom_left && bottom_right;
}

fn rounded_rect_corner_fully_covers(point: vec2<f32>, corner: vec2<f32>, center: vec2<f32>, radius: f32) -> bool {
    let in_corner_square = radius > 0.0 &&
        abs(point.x - corner.x) < radius &&
        abs(point.y - corner.y) < radius;
    let inner_radius = radius - FULL_TILE_SDF_SOLID_INSET;
    let delta = point - center;
    return !in_corner_square || (inner_radius > 0.0 && dot(delta, delta) <= inner_radius * inner_radius);
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
        coarse_work[base + 6u],
    );
}

fn store_emit_chunk_ref(ref_ix: u32, tile_ix: u32, local_chunk: u32) {
    let base = emit_chunk_record_base(ref_ix);
    coarse_work[base] = tile_ix;
    coarse_work[base + 1u] = local_chunk;
    coarse_work[base + 6u] = 0u;
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

fn store_emit_chunk_class_flags(ref_ix: u32, flags: u32) {
    coarse_work[emit_chunk_record_base(ref_ix) + 6u] = flags;
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
