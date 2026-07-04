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
    _pad0: u32,
    _pad1: u32,
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
@group(0) @binding(1) var<storage, read> draw_records: array<DrawRecord>;
@group(0) @binding(3) var<storage, read> text_runs: array<GlyphRunRecord>;
@group(0) @binding(5) var<storage, read> glyphs: array<GlyphRecord>;
@group(0) @binding(8) var<storage, read> glyph_images: array<GlyphImageRecord>;
@group(0) @binding(13) var<storage, read> brush_blob: array<u32>;
@group(0) @binding(18) var<storage, read> path_records: array<PathRecord>;
@group(0) @binding(19) var<storage, read_write> backdrops: array<atomic<i32>>;
@group(0) @binding(20) var<storage, read> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(22) var<storage, read> layer_stack: array<LayerStackRecord>;
@group(0) @binding(25) var<storage, read_write> tile_records: array<TileCoarseRecord>;
@group(0) @binding(31) var<storage, read_write> chunk_records: array<CoarseChunkRecord>;
@group(0) @binding(35) var<storage, read_write> ptcl_records: array<PtclRecord>;
@group(0) @binding(41) var<storage, read_write> glyph_indices: array<u32>;
@group(0) @binding(42) var<storage, read> tile_draw_data: array<u32>;

const INVALID: u32 = 0xffffffffu;
const GPU_DRAW_BRUSH: u32 = 0u;
const GPU_DRAW_CLIP: u32 = 1u;
const GPU_DRAW_OPACITY: u32 = 2u;
const GPU_DRAW_BLEND: u32 = 3u;
const GPU_DRAW_ISOLATE: u32 = 4u;
const GPU_DRAW_PATH_GLYPH: u32 = 5u;
const GPU_BRUSH_U32_STRIDE: u32 = 9u;
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

var<workgroup> coarse_scratch: array<u32, 256>;
var<workgroup> coarse_total: u32;

@compute @workgroup_size(256)
fn coarse_count(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = workgroup_id.x;
    if (tile_ix >= config.tile_count) {
        return;
    }

    let lane = local_id.x;
    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    let wrapper_count = active_stack_count(tile_x, tile_y);
    var count = 0u;
    var glyph_count = 0u;

    if (wrapper_count != INVALID) {
        let tile_draw_start = tile_draw_start_at(tile_ix);
        let tile_draw_end = tile_draw_end_at(tile_ix);
        var draw_ref_ix = tile_draw_start + lane;
        loop {
            if (draw_ref_ix >= tile_draw_end) {
                break;
            }
            let draw_ix = tile_draw_index_at(draw_ref_ix);
            if (draw_in_batch(draw_ix)) {
                let draw_tag = draw_tag_at(draw_ix);
                if (draw_has_glyph_at(draw_ix)) {
                    if (draw_tag == GPU_DRAW_BRUSH) {
                        let tile_glyphs = count_tile_glyphs_for_run(draw_records[draw_ix].glyph_run_id, tile_x, tile_y);
                        if (tile_glyphs > 0u) {
                            count += 1u;
                            glyph_count += tile_glyphs;
                        }
                    }
                } else if (draw_has_sdf_at(draw_ix)) {
                    if (draw_tag == GPU_DRAW_BRUSH) {
                        count += 1u;
                    }
                } else {
                    let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
                    if (backdrop_ix != INVALID) {
                        if (
                            (draw_tag == GPU_DRAW_BRUSH || draw_tag == GPU_DRAW_PATH_GLYPH || draw_tag == GPU_DRAW_CLIP) &&
                            (segment_ranges[backdrop_ix].start != segment_ranges[backdrop_ix].end || atomicLoad(&backdrops[backdrop_ix]) != 0i)
                        ) {
                            count += 1u;
                        }
                    }
                }
            }
            draw_ref_ix += 256u;
        }
    }

    let tile_ptcl_count = workgroup_sum(count, lane);
    let tile_glyph_count = workgroup_sum(glyph_count, lane);
    if (lane == 0u) {
        var stored_count = tile_ptcl_count;
        if (tile_ptcl_count > 0u) {
            stored_count = tile_ptcl_count + wrapper_count * 2u + 1u;
        }
        tile_records[tile_ix].ptcl_count = stored_count;
        tile_records[tile_ix].glyph_count = tile_glyph_count;
    }
}

fn workgroup_sum(value: u32, lane: u32) -> u32 {
    _ = workgroup_exclusive_prefix(value, lane);
    return coarse_total;
}

// Each tile now scans its pre-binned draw references instead of the whole draw
// table. The prefix sum still preserves draw-order offsets inside each tile.
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

@compute @workgroup_size(256)
fn coarse_emit(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = workgroup_id.x;
    if (tile_ix >= config.tile_count) {
        return;
    }

    let lane = local_id.x;
    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    let tile = tile_records[tile_ix];
    var cursor = tile.ptcl_start;
    let range_end = tile.ptcl_end;
    var glyph_cursor = tile.glyph_start;
    let glyph_range_end = tile.glyph_end;
    if (cursor >= range_end) {
        return;
    }

    let wrapper_count = active_stack_count(tile_x, tile_y);
    if (wrapper_count == INVALID) {
        return;
    }

    if (lane == 0u) {
        emit_active_stack_begins(cursor, tile_x, tile_y);
    }
    cursor += wrapper_count;

    let tile_draw_start = tile_draw_start_at(tile_ix);
    let tile_draw_end = tile_draw_end_at(tile_ix);
    var chunk_start = tile_draw_start;
    loop {
        if (chunk_start >= tile_draw_end) {
            break;
        }

        let draw_ref_ix = chunk_start + lane;
        var draw_ix = INVALID;
        var valid = false;
        var glyph_count = 0u;
        var ptcl_tag = GPU_PTCL_FILL;
        var ptcl_backdrop = 0i;
        var ptcl_fill_rule = 0u;
        var ptcl_segment_start = 0u;
        var ptcl_segment_end = 0u;
        var ptcl_color = 0u;

        if (draw_ref_ix < tile_draw_end) {
            draw_ix = tile_draw_index_at(draw_ref_ix);
        }

        if (draw_in_batch(draw_ix)) {
            let draw_tag = draw_tag_at(draw_ix);
            if (draw_has_glyph_at(draw_ix)) {
                if (draw_tag == GPU_DRAW_BRUSH) {
                    glyph_count = count_tile_glyphs_for_run(draw_records[draw_ix].glyph_run_id, tile_x, tile_y);
                    if (glyph_count > 0u) {
                        valid = true;
                        ptcl_tag = GPU_PTCL_GLYPH;
                        ptcl_color = draw_ix;
                    }
                }
            } else if (draw_has_sdf_at(draw_ix)) {
                if (draw_tag == GPU_DRAW_BRUSH) {
                    valid = true;
                    ptcl_tag = GPU_PTCL_SDF;
                    ptcl_segment_start = draw_ix;
                    ptcl_color = draw_ix;
                }
            } else {
                let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
                if (backdrop_ix != INVALID) {
                    let segment_range = segment_ranges[backdrop_ix];
                    let segment_start = segment_range.start;
                    let segment_end = segment_range.end;
                    let backdrop = atomicLoad(&backdrops[backdrop_ix]);
                    if (
                        (draw_tag == GPU_DRAW_BRUSH || draw_tag == GPU_DRAW_PATH_GLYPH || draw_tag == GPU_DRAW_CLIP) &&
                        (segment_start != segment_end || backdrop != 0i)
                    ) {
                        if (draw_tag == GPU_DRAW_CLIP) {
                            ptcl_tag = GPU_PTCL_BEGIN_CLIP;
                        } else if (draw_tag == GPU_DRAW_PATH_GLYPH) {
                            ptcl_tag = GPU_PTCL_PATH_GLYPH;
                        } else if (draw_solid_color_fast_path_at(draw_ix) && segment_start == segment_end) {
                            ptcl_tag = GPU_PTCL_COLOR;
                        }
                        valid = true;
                        ptcl_backdrop = backdrop;
                        ptcl_fill_rule = draw_fill_rule_at(draw_ix);
                        ptcl_segment_start = segment_start;
                        ptcl_segment_end = segment_end;
                        if (ptcl_tag == GPU_PTCL_COLOR) {
                            ptcl_color = draw_solid_color_at(draw_ix);
                        } else if (draw_tag == GPU_DRAW_BRUSH || draw_tag == GPU_DRAW_PATH_GLYPH) {
                            ptcl_color = draw_ix;
                        }
                    }
                }
            }
        }

        let valid_count = select(0u, 1u, valid);
        let particle_offset = workgroup_exclusive_prefix(valid_count, lane);
        let emitted = coarse_total;
        let glyph_offset = workgroup_exclusive_prefix(glyph_count, lane);
        let emitted_glyphs = coarse_total;
        if (valid) {
            if (ptcl_tag == GPU_PTCL_GLYPH) {
                ptcl_segment_start = glyph_cursor + glyph_offset;
                ptcl_segment_end = ptcl_segment_start + glyph_count;
                if (ptcl_segment_end <= glyph_range_end) {
                    store_tile_glyphs_for_run(ptcl_segment_start, draw_records[draw_ix].glyph_run_id, tile_x, tile_y);
                }
            }
            store_particle(
                cursor + particle_offset,
                ptcl_tag,
                ptcl_backdrop,
                ptcl_fill_rule,
                ptcl_segment_start,
                ptcl_segment_end,
                ptcl_color
            );
        }
        cursor += emitted;
        glyph_cursor += emitted_glyphs;
        chunk_start += 256u;
    }

    if (lane == 0u) {
        emit_active_stack_ends(cursor);
        store_particle(cursor + wrapper_count, GPU_PTCL_END, 0i, 0u, 0u, 0u, 0u);
    }
}

fn active_stack_count(tile_x: u32, tile_y: u32) -> u32 {
    var count = 0u;
    var valid = true;
    var stack_ix = config.layer_stack_start;
    loop {
        if (stack_ix >= config.layer_stack_end) {
            break;
        }
        if (valid) {
            let layer = layer_stack[stack_ix];
            let layer_tag = layer.tag;
            if (layer_tag != GPU_LAYER_CLIP && layer_tag != GPU_LAYER_OPACITY && layer_tag != GPU_LAYER_BLEND) {
                valid = false;
            } else {
                let draw_ix = layer.draw;
                if (draw_has_sdf_at(draw_ix)) {
                    if (draw_tile_hit(draw_ix, tile_x, tile_y)) {
                        count += 1u;
                    } else {
                        valid = false;
                    }
                } else {
                    let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
                    if (backdrop_ix == INVALID) {
                        valid = false;
                    } else if (segment_ranges[backdrop_ix].start == segment_ranges[backdrop_ix].end && atomicLoad(&backdrops[backdrop_ix]) == 0i) {
                        valid = false;
                    } else {
                        count += 1u;
                    }
                }
            }
        }
        stack_ix += 1u;
    }
    if (valid) {
        return count;
    }
    return INVALID;
}

fn emit_active_stack_begins(dst_start: u32, tile_x: u32, tile_y: u32) {
    var stack_ix = config.layer_stack_start;
    var dst = dst_start;
    loop {
        if (stack_ix >= config.layer_stack_end) {
            break;
        }
        let layer = layer_stack[stack_ix];
        let layer_tag = layer.tag;
        if (layer_tag == GPU_LAYER_CLIP || layer_tag == GPU_LAYER_OPACITY || layer_tag == GPU_LAYER_BLEND) {
            let draw_ix = layer.draw;
            if (layer_tag == GPU_LAYER_CLIP && draw_has_sdf_at(draw_ix)) {
                store_particle(dst, GPU_PTCL_BEGIN_SDF_CLIP, 0i, 0u, 0u, 0u, draw_ix);
                dst += 1u;
            } else {
                let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
                if (backdrop_ix != INVALID) {
                    var ptcl_tag = GPU_PTCL_BEGIN_CLIP;
                    if (layer_tag == GPU_LAYER_OPACITY) {
                        ptcl_tag = GPU_PTCL_BEGIN_OPACITY;
                    } else if (layer_tag == GPU_LAYER_BLEND) {
                        ptcl_tag = GPU_PTCL_BEGIN_BLEND;
                    }
                    store_particle(
                        dst,
                        ptcl_tag,
                        atomicLoad(&backdrops[backdrop_ix]),
                        draw_fill_rule_at(draw_ix),
                        segment_ranges[backdrop_ix].start,
                        segment_ranges[backdrop_ix].end,
                        layer.payload
                    );
                    dst += 1u;
                }
            }
        }
        stack_ix += 1u;
    }
}

fn emit_active_stack_ends(dst_start: u32) {
    var stack_ix = config.layer_stack_end;
    var dst = dst_start;
    loop {
        if (stack_ix <= config.layer_stack_start) {
            break;
        }
        stack_ix -= 1u;
        let layer_tag = layer_stack[stack_ix].tag;
        var ptcl_tag = GPU_PTCL_END_CLIP;
        var valid = true;
        if (layer_tag == GPU_LAYER_OPACITY) {
            ptcl_tag = GPU_PTCL_END_OPACITY;
        } else if (layer_tag == GPU_LAYER_BLEND) {
            ptcl_tag = GPU_PTCL_END_BLEND;
        } else if (layer_tag != GPU_LAYER_CLIP) {
            valid = false;
        }
        if (valid) {
            store_particle(dst, ptcl_tag, 0i, 0u, 0u, 0u, 0u);
            dst += 1u;
        }
    }
}

fn draw_backdrop_ix(draw_ix: u32, tile_x: u32, tile_y: u32) -> u32 {
    let draw = draw_records[draw_ix];
    let path_id = draw.path_id;
    let draw_tag = draw_tag_at(draw_ix);
    var result = INVALID;
    if (
        path_id != INVALID &&
        (draw_tag == GPU_DRAW_BRUSH ||
         draw_tag == GPU_DRAW_PATH_GLYPH ||
         draw_tag == GPU_DRAW_CLIP ||
         draw_tag == GPU_DRAW_OPACITY ||
         draw_tag == GPU_DRAW_BLEND ||
         draw_tag == GPU_DRAW_ISOLATE)
    ) {
        let draw_x0 = pixel_tile_min(draw.pixel_x0, config.tiles_width);
        let draw_y0 = pixel_tile_min(draw.pixel_y0, config.tiles_height);
        let draw_x1 = pixel_tile_max(draw.pixel_x1, config.tiles_width);
        let draw_y1 = pixel_tile_max(draw.pixel_y1, config.tiles_height);
        if (tile_x >= draw_x0 && tile_x < draw_x1 && tile_y >= draw_y0 && tile_y < draw_y1 && path_id < arrayLength(&path_records)) {
            let path = path_records[path_id];
            let bx0 = path.tile_x0;
            let by0 = path.tile_y0;
            let bx1 = path.tile_x1;
            let by1 = path.tile_y1;
            let stride = bx1 - bx0;
            if (stride > 0u && tile_x >= bx0 && tile_x < bx1 && tile_y >= by0 && tile_y < by1) {
                let local_ix = (tile_y - by0) * stride + tile_x - bx0;
                result = path.data_offset + local_ix;
            }
        }
    }
    return result;
}

fn draw_tile_hit(draw_ix: u32, tile_x: u32, tile_y: u32) -> bool {
    let draw = draw_records[draw_ix];
    let draw_x0 = pixel_tile_min(draw.pixel_x0, config.tiles_width);
    let draw_y0 = pixel_tile_min(draw.pixel_y0, config.tiles_height);
    let draw_x1 = pixel_tile_max(draw.pixel_x1, config.tiles_width);
    let draw_y1 = pixel_tile_max(draw.pixel_y1, config.tiles_height);
    return tile_x >= draw_x0 && tile_x < draw_x1 && tile_y >= draw_y0 && tile_y < draw_y1;
}

fn count_tile_glyphs_for_run(run_id: u32, tile_x: u32, tile_y: u32) -> u32 {
    var count = 0u;
    let run = text_runs[run_id];
    var glyph_ix = run.glyph_start;
    let glyph_end = glyph_ix + run.glyph_count;
    loop {
        if (glyph_ix >= glyph_end) {
            break;
        }
        if (glyph_hits_tile(glyph_ix, tile_x, tile_y)) {
            count += 1u;
        }
        glyph_ix += 1u;
    }
    return count;
}

fn store_tile_glyphs_for_run(dst_start: u32, run_id: u32, tile_x: u32, tile_y: u32) {
    var count = 0u;
    let run = text_runs[run_id];
    var glyph_ix = run.glyph_start;
    let glyph_end = glyph_ix + run.glyph_count;
    loop {
        if (glyph_ix >= glyph_end) {
            break;
        }
        if (glyph_hits_tile(glyph_ix, tile_x, tile_y)) {
            let dst = dst_start + count;
            if (dst < config.glyph_capacity) {
                glyph_indices[dst] = glyph_ix;
            }
            count += 1u;
        }
        glyph_ix += 1u;
    }
}

fn glyph_hits_tile(glyph_ix: u32, tile_x: u32, tile_y: u32) -> bool {
    let glyph = glyphs[glyph_ix];
    let image_id = glyph.image_id;
    if (image_id == INVALID) {
        return false;
    }
    let image = glyph_images[image_id];
    let width = image.width;
    let height = image.height;
    if (width == 0u || height == 0u) {
        return false;
    }
    let x0 = glyph.x + image.left;
    let y0 = glyph.y - image.top;
    let x1 = x0 + i32(width);
    let y1 = y0 + i32(height);
    let tile_x0 = i32(tile_x * 16u);
    let tile_y0 = i32(tile_y * 16u);
    let tile_x1 = tile_x0 + 16i;
    let tile_y1 = tile_y0 + 16i;
    return x0 < tile_x1 && x1 > tile_x0 && y0 < tile_y1 && y1 > tile_y0;
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

fn draw_in_batch(draw_ix: u32) -> bool {
    return draw_ix >= config.draw_start && draw_ix < config.draw_end;
}

fn tile_draw_start_at(tile_ix: u32) -> u32 {
    return tile_draw_data[tile_ix * 2u];
}

fn tile_draw_end_at(tile_ix: u32) -> u32 {
    return tile_draw_data[tile_ix * 2u + 1u];
}

fn tile_draw_index_at(draw_ref_ix: u32) -> u32 {
    return tile_draw_data[config.tile_count * 2u + draw_ref_ix];
}

fn draw_tag_at(draw_ix: u32) -> u32 {
    return draw_records[draw_ix].tag;
}

fn draw_fill_rule_at(draw_ix: u32) -> u32 {
    return draw_records[draw_ix].fill_rule;
}

fn draw_has_sdf_at(draw_ix: u32) -> bool {
    let draw = draw_records[draw_ix];
    return draw.sdf_offset != INVALID || draw.sdf_shadow_offset != INVALID;
}

fn draw_has_glyph_at(draw_ix: u32) -> bool {
    return draw_records[draw_ix].glyph_run_id != INVALID;
}

fn draw_solid_color_fast_path_at(draw_ix: u32) -> bool {
    let draw = draw_records[draw_ix];
    let brush_base = draw.brush_offset;
    return draw_records[draw_ix].solid_rect != 0u &&
        brush_base != INVALID &&
        brush_blob[brush_base] == GPU_BRUSH_SOLID &&
        brush_blob[brush_base + 4u] != 0u;
}

fn draw_solid_color_at(draw_ix: u32) -> u32 {
    return brush_blob[draw_records[draw_ix].brush_offset + 4u];
}

fn store_particle(dst: u32, tag: u32, backdrop: i32, fill_rule: u32, segment_start: u32, segment_end: u32, color: u32) {
    if (dst < config.ptcl_capacity) {
        ptcl_records[dst].tag = tag;
        ptcl_records[dst].backdrop = backdrop;
        ptcl_records[dst].fill_rule = fill_rule;
        ptcl_records[dst].segment_start = segment_start;
        ptcl_records[dst].segment_end = segment_end;
        ptcl_records[dst].color = color;
    }
}
