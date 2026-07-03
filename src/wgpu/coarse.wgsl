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
@group(0) @binding(1) var<storage, read> draw_path_ids: array<u32>;
@group(0) @binding(2) var<storage, read> draw_glyph_run_ids: array<u32>;
@group(0) @binding(3) var<storage, read> glyph_run_starts: array<u32>;
@group(0) @binding(4) var<storage, read> glyph_run_counts: array<u32>;
@group(0) @binding(5) var<storage, read> glyph_image_ids: array<u32>;
@group(0) @binding(6) var<storage, read> glyph_x: array<i32>;
@group(0) @binding(7) var<storage, read> glyph_y: array<i32>;
@group(0) @binding(8) var<storage, read> glyph_image_left: array<i32>;
@group(0) @binding(9) var<storage, read> glyph_image_top: array<i32>;
@group(0) @binding(10) var<storage, read> glyph_image_width: array<u32>;
@group(0) @binding(11) var<storage, read> glyph_image_height: array<u32>;
@group(0) @binding(12) var<storage, read> draw_flags: array<u32>;
@group(0) @binding(13) var<storage, read> draw_brush_colors: array<u32>;
@group(0) @binding(14) var<storage, read> draw_pixel_x0: array<i32>;
@group(0) @binding(15) var<storage, read> draw_pixel_y0: array<i32>;
@group(0) @binding(16) var<storage, read> draw_pixel_x1: array<i32>;
@group(0) @binding(17) var<storage, read> draw_pixel_y1: array<i32>;
@group(0) @binding(18) var<storage, read> backdrop_data_offsets: array<u32>;
@group(0) @binding(19) var<storage, read> backdrop_tile_x0: array<u32>;
@group(0) @binding(20) var<storage, read> backdrop_tile_y0: array<u32>;
@group(0) @binding(21) var<storage, read> backdrop_tile_x1: array<u32>;
@group(0) @binding(22) var<storage, read> backdrop_tile_y1: array<u32>;
@group(0) @binding(23) var<storage, read_write> backdrops: array<atomic<i32>>;
@group(0) @binding(24) var<storage, read> segment_starts: array<u32>;
@group(0) @binding(25) var<storage, read> segment_ends: array<u32>;
@group(0) @binding(26) var<storage, read> layer_stack_tags: array<u32>;
@group(0) @binding(27) var<storage, read> layer_stack_draws: array<u32>;
@group(0) @binding(28) var<storage, read> layer_stack_payloads: array<u32>;
@group(0) @binding(29) var<storage, read_write> tile_ptcl_counts: array<u32>;
@group(0) @binding(30) var<storage, read_write> tile_ptcl_range_starts: array<u32>;
@group(0) @binding(31) var<storage, read_write> tile_ptcl_range_ends: array<u32>;
@group(0) @binding(32) var<storage, read_write> tile_glyph_counts: array<u32>;
@group(0) @binding(33) var<storage, read_write> tile_glyph_range_starts: array<u32>;
@group(0) @binding(34) var<storage, read_write> tile_glyph_range_ends: array<u32>;
@group(0) @binding(35) var<storage, read_write> chunk_totals: array<u32>;
@group(0) @binding(36) var<storage, read_write> chunk_offsets: array<u32>;
@group(0) @binding(37) var<storage, read_write> glyph_chunk_totals: array<u32>;
@group(0) @binding(38) var<storage, read_write> glyph_chunk_offsets: array<u32>;
@group(0) @binding(39) var<storage, read_write> ptcl_tags: array<atomic<u32>>;
@group(0) @binding(40) var<storage, read_write> ptcl_backdrops: array<i32>;
@group(0) @binding(41) var<storage, read_write> ptcl_fill_rules: array<u32>;
@group(0) @binding(42) var<storage, read_write> ptcl_segment_starts: array<u32>;
@group(0) @binding(43) var<storage, read_write> ptcl_segment_ends: array<u32>;
@group(0) @binding(44) var<storage, read_write> ptcl_colors: array<u32>;
@group(0) @binding(45) var<storage, read_write> glyph_indices: array<u32>;

const INVALID: u32 = 0xffffffffu;
const GPU_DRAW_BRUSH: u32 = 0u;
const GPU_DRAW_CLIP: u32 = 1u;
const GPU_DRAW_OPACITY: u32 = 2u;
const GPU_DRAW_BLEND: u32 = 3u;
const GPU_DRAW_ISOLATE: u32 = 4u;
const GPU_DRAW_PATH_GLYPH: u32 = 5u;
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
const DRAW_FLAG_TAG_MASK: u32 = 7u;
const DRAW_FLAG_FILL_RULE_EVEN_ODD: u32 = 8u;
const DRAW_FLAG_SOLID_COLOR_FAST_PATH: u32 = 32u;
const DRAW_FLAG_HAS_SDF: u32 = 64u;
const DRAW_FLAG_HAS_GLYPH: u32 = 128u;

var<workgroup> coarse_scratch: array<u32, 256>;

@compute @workgroup_size(256)
fn coarse_count(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = workgroup_id.x;
    if (tile_ix >= config.tile_count || local_id.x != 0u) {
        return;
    }

    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    let wrapper_count = active_stack_count(tile_x, tile_y);
    var count = 0u;
    var glyph_count = 0u;

    if (wrapper_count != INVALID) {
        var draw_ix = config.draw_start;
        loop {
            if (draw_ix >= config.draw_end) {
                break;
            }
            let draw_tag = draw_tag_at(draw_ix);
            if (draw_has_glyph_at(draw_ix)) {
                if (draw_tag == GPU_DRAW_BRUSH && draw_tile_hit(draw_ix, tile_x, tile_y)) {
                    let tile_glyphs = count_tile_glyphs_for_run(draw_glyph_run_ids[draw_ix], tile_x, tile_y);
                    if (tile_glyphs > 0u) {
                        count += 1u;
                        glyph_count += tile_glyphs;
                    }
                }
            } else if (draw_has_sdf_at(draw_ix)) {
                if (draw_tag == GPU_DRAW_BRUSH && draw_tile_hit(draw_ix, tile_x, tile_y)) {
                    count += 1u;
                }
            } else {
                let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
                if (backdrop_ix != INVALID) {
                    if (
                        (draw_tag == GPU_DRAW_BRUSH || draw_tag == GPU_DRAW_PATH_GLYPH || draw_tag == GPU_DRAW_CLIP) &&
                        (segment_starts[backdrop_ix] != segment_ends[backdrop_ix] || atomicLoad(&backdrops[backdrop_ix]) != 0i)
                    ) {
                        count += 1u;
                    }
                }
            }
            draw_ix += 1u;
        }
    }

    tile_glyph_counts[tile_ix] = glyph_count;
    var stored_count = count;
    if (count > 0u) {
        stored_count = count + wrapper_count * 2u + 1u;
    }
    tile_ptcl_counts[tile_ix] = stored_count;
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
            count = tile_glyph_counts[tile_ix];
        } else {
            count = tile_ptcl_counts[tile_ix];
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
            glyph_chunk_totals[chunk_ix] = coarse_scratch[255u];
        } else {
            chunk_totals[chunk_ix] = coarse_scratch[255u];
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
            tile_glyph_range_starts[tile_ix] = start;
            tile_glyph_range_ends[tile_ix] = start + count;
        } else {
            tile_ptcl_range_starts[tile_ix] = start;
            tile_ptcl_range_ends[tile_ix] = start + count;
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
        chunk_offsets[chunk_ix] = carry;
        carry += chunk_totals[chunk_ix];
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
        glyph_chunk_offsets[chunk_ix] = carry;
        carry += glyph_chunk_totals[chunk_ix];
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
        let offset = glyph_chunk_offsets[chunk_ix];
        tile_glyph_range_starts[tile_ix] += offset;
        tile_glyph_range_ends[tile_ix] += offset;
    } else {
        let offset = chunk_offsets[chunk_ix];
        tile_ptcl_range_starts[tile_ix] += offset;
        tile_ptcl_range_ends[tile_ix] += offset;
    }
}

@compute @workgroup_size(256)
fn coarse_emit(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = workgroup_id.x;
    if (tile_ix >= config.tile_count || local_id.x != 0u) {
        return;
    }

    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    var cursor = tile_ptcl_range_starts[tile_ix];
    let range_end = tile_ptcl_range_ends[tile_ix];
    var glyph_cursor = tile_glyph_range_starts[tile_ix];
    let glyph_range_end = tile_glyph_range_ends[tile_ix];
    if (cursor >= range_end) {
        return;
    }

    let wrapper_count = active_stack_count(tile_x, tile_y);
    if (wrapper_count == INVALID) {
        return;
    }

    emit_active_stack_begins(cursor, tile_x, tile_y);
    cursor += wrapper_count;

    var draw_ix = config.draw_start;
    loop {
        if (draw_ix >= config.draw_end) {
            break;
        }

        var valid = false;
        var glyph_count = 0u;
        var ptcl_tag = GPU_PTCL_FILL;
        var ptcl_backdrop = 0i;
        var ptcl_fill_rule = 0u;
        var ptcl_segment_start = 0u;
        var ptcl_segment_end = 0u;
        var ptcl_color = 0u;
        let draw_tag = draw_tag_at(draw_ix);

        if (draw_has_glyph_at(draw_ix)) {
            if (draw_tag == GPU_DRAW_BRUSH && draw_tile_hit(draw_ix, tile_x, tile_y)) {
                glyph_count = count_tile_glyphs_for_run(draw_glyph_run_ids[draw_ix], tile_x, tile_y);
                if (glyph_count > 0u) {
                    valid = true;
                    ptcl_tag = GPU_PTCL_GLYPH;
                    ptcl_color = draw_ix;
                }
            }
        } else if (draw_has_sdf_at(draw_ix)) {
            if (draw_tag == GPU_DRAW_BRUSH && draw_tile_hit(draw_ix, tile_x, tile_y)) {
                valid = true;
                ptcl_tag = GPU_PTCL_SDF;
                ptcl_segment_start = draw_ix;
                ptcl_color = draw_ix;
            }
        } else {
            let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
            if (backdrop_ix != INVALID) {
                let segment_start = segment_starts[backdrop_ix];
                let segment_end = segment_ends[backdrop_ix];
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
                        ptcl_color = draw_brush_colors[draw_ix];
                    } else if (draw_tag == GPU_DRAW_BRUSH || draw_tag == GPU_DRAW_PATH_GLYPH) {
                        ptcl_color = draw_ix;
                    }
                }
            }
        }

        if (valid) {
            if (ptcl_tag == GPU_PTCL_GLYPH) {
                ptcl_segment_start = glyph_cursor;
                ptcl_segment_end = ptcl_segment_start + glyph_count;
                if (ptcl_segment_end <= glyph_range_end) {
                    store_tile_glyphs_for_run(ptcl_segment_start, draw_glyph_run_ids[draw_ix], tile_x, tile_y);
                }
                glyph_cursor += glyph_count;
            }
            store_particle(
                cursor,
                ptcl_tag,
                ptcl_backdrop,
                ptcl_fill_rule,
                ptcl_segment_start,
                ptcl_segment_end,
                ptcl_color
            );
            cursor += 1u;
        }

        draw_ix += 1u;
    }

    emit_active_stack_ends(cursor);
    store_particle(cursor + wrapper_count, GPU_PTCL_END, 0i, 0u, 0u, 0u, 0u);
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
            let layer_tag = layer_stack_tags[stack_ix];
            if (layer_tag != GPU_LAYER_CLIP && layer_tag != GPU_LAYER_OPACITY && layer_tag != GPU_LAYER_BLEND) {
                valid = false;
            } else {
                let draw_ix = layer_stack_draws[stack_ix];
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
                    } else if (segment_starts[backdrop_ix] == segment_ends[backdrop_ix] && atomicLoad(&backdrops[backdrop_ix]) == 0i) {
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
        let layer_tag = layer_stack_tags[stack_ix];
        if (layer_tag == GPU_LAYER_CLIP || layer_tag == GPU_LAYER_OPACITY || layer_tag == GPU_LAYER_BLEND) {
            let draw_ix = layer_stack_draws[stack_ix];
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
                        segment_starts[backdrop_ix],
                        segment_ends[backdrop_ix],
                        layer_stack_payloads[stack_ix]
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
        let layer_tag = layer_stack_tags[stack_ix];
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
    let path_id = draw_path_ids[draw_ix];
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
        let draw_x0 = pixel_tile_min(draw_pixel_x0[draw_ix], config.tiles_width);
        let draw_y0 = pixel_tile_min(draw_pixel_y0[draw_ix], config.tiles_height);
        let draw_x1 = pixel_tile_max(draw_pixel_x1[draw_ix], config.tiles_width);
        let draw_y1 = pixel_tile_max(draw_pixel_y1[draw_ix], config.tiles_height);
        if (tile_x >= draw_x0 && tile_x < draw_x1 && tile_y >= draw_y0 && tile_y < draw_y1 && path_id < arrayLength(&backdrop_data_offsets)) {
            let bx0 = backdrop_tile_x0[path_id];
            let by0 = backdrop_tile_y0[path_id];
            let bx1 = backdrop_tile_x1[path_id];
            let by1 = backdrop_tile_y1[path_id];
            let stride = bx1 - bx0;
            if (stride > 0u && tile_x >= bx0 && tile_x < bx1 && tile_y >= by0 && tile_y < by1) {
                let local_ix = (tile_y - by0) * stride + tile_x - bx0;
                result = backdrop_data_offsets[path_id] + local_ix;
            }
        }
    }
    return result;
}

fn draw_tile_hit(draw_ix: u32, tile_x: u32, tile_y: u32) -> bool {
    let draw_x0 = pixel_tile_min(draw_pixel_x0[draw_ix], config.tiles_width);
    let draw_y0 = pixel_tile_min(draw_pixel_y0[draw_ix], config.tiles_height);
    let draw_x1 = pixel_tile_max(draw_pixel_x1[draw_ix], config.tiles_width);
    let draw_y1 = pixel_tile_max(draw_pixel_y1[draw_ix], config.tiles_height);
    return tile_x >= draw_x0 && tile_x < draw_x1 && tile_y >= draw_y0 && tile_y < draw_y1;
}

fn count_tile_glyphs_for_run(run_id: u32, tile_x: u32, tile_y: u32) -> u32 {
    var count = 0u;
    var glyph_ix = glyph_run_starts[run_id];
    let glyph_end = glyph_ix + glyph_run_counts[run_id];
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
    var glyph_ix = glyph_run_starts[run_id];
    let glyph_end = glyph_ix + glyph_run_counts[run_id];
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
    let image_id = glyph_image_ids[glyph_ix];
    if (image_id == INVALID) {
        return false;
    }
    let width = glyph_image_width[image_id];
    let height = glyph_image_height[image_id];
    if (width == 0u || height == 0u) {
        return false;
    }
    let x0 = glyph_x[glyph_ix] + glyph_image_left[image_id];
    let y0 = glyph_y[glyph_ix] - glyph_image_top[image_id];
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

fn draw_flags_at(draw_ix: u32) -> u32 {
    return draw_flags[draw_ix];
}

fn draw_tag_at(draw_ix: u32) -> u32 {
    return draw_flags_at(draw_ix) & DRAW_FLAG_TAG_MASK;
}

fn draw_fill_rule_at(draw_ix: u32) -> u32 {
    return (draw_flags_at(draw_ix) & DRAW_FLAG_FILL_RULE_EVEN_ODD) >> 3u;
}

fn draw_has_sdf_at(draw_ix: u32) -> bool {
    return (draw_flags_at(draw_ix) & DRAW_FLAG_HAS_SDF) != 0u;
}

fn draw_has_glyph_at(draw_ix: u32) -> bool {
    return (draw_flags_at(draw_ix) & DRAW_FLAG_HAS_GLYPH) != 0u;
}

fn draw_solid_color_fast_path_at(draw_ix: u32) -> bool {
    return (draw_flags_at(draw_ix) & DRAW_FLAG_SOLID_COLOR_FAST_PATH) != 0u;
}

fn store_particle(dst: u32, tag: u32, backdrop: i32, fill_rule: u32, segment_start: u32, segment_end: u32, color: u32) {
    if (dst < config.ptcl_capacity) {
        store_packed_atomic_u8(dst, tag);
        ptcl_backdrops[dst] = backdrop;
        ptcl_fill_rules[dst] = fill_rule;
        ptcl_segment_starts[dst] = segment_start;
        ptcl_segment_ends[dst] = segment_end;
        ptcl_colors[dst] = color;
    }
}

fn store_packed_atomic_u8(ix: u32, value: u32) {
    let shift = (ix % 4u) * 8u;
    let mask = 255u << shift;
    let word_ix = ix / 4u;
    atomicAnd(&ptcl_tags[word_ix], 0xffffffffu - mask);
    if (value != 0u) {
        atomicOr(&ptcl_tags[word_ix], (value & 255u) << shift);
    }
}
