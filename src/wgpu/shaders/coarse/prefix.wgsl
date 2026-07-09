#include "common.wgsl"
#include "text_input.wgsl"

@group(0) @binding(1) var<storage, read> draw_records: array<DrawRecord>;
@group(0) @binding(2) var<storage, read> text_blob: array<u32>;
@group(0) @binding(3) var<storage, read> path_records: array<PathRecord>;
@group(0) @binding(4) var<storage, read_write> backdrops: array<atomic<i32>>;
@group(0) @binding(5) var<storage, read> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(6) var<storage, read> layer_stack: array<LayerStackRecord>;
@group(0) @binding(7) var<storage, read_write> coarse_work: array<u32>;
@group(0) @binding(8) var<storage, read_write> chunk_records: array<CoarseChunkRecord>;
@group(0) @binding(9) var<storage, read> sdf_blob: array<u32>;

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
        count = coarse_tile_count(tile_ix, glyph);
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
        coarse_store_tile_range(tile_ix, glyph, start, start + count);
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
        coarse_add_tile_range_offset(tile_ix, true, offset);
    } else {
        let offset = chunk_records[chunk_ix].ptcl_offset;
        coarse_add_tile_range_offset(tile_ix, false, offset);
    }
}

@compute @workgroup_size(256)
fn coarse_emit_chunk_counts(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = workgroup_id.x * 256u + local_id.x;
    if (tile_ix >= config.tile_count) {
        return;
    }
    let draw_count = tile_draw_end_at(tile_ix) - tile_draw_start_at(tile_ix);
    store_tile_emit_chunk_count(tile_ix, (draw_count + 255u) / 256u);
}

@compute @workgroup_size(256)
fn coarse_emit_prefix_chunks(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let chunk_ix = workgroup_id.x;
    let lane = local_id.x;
    let chunk_offset = chunk_ix * 256u;
    let chunk_len = min(config.tile_count - chunk_offset, 256u);
    var count = 0u;
    if (lane < chunk_len) {
        count = tile_emit_chunk_count_at(chunk_offset + lane);
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
        chunk_records[chunk_ix].ptcl_total = coarse_scratch[255u];
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
        store_tile_emit_chunk_offset(chunk_offset + lane, coarse_scratch[lane]);
    }
}

@compute @workgroup_size(1)
fn coarse_emit_chunk_offsets() {
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

@compute @workgroup_size(256)
fn coarse_emit_apply_chunk_offsets(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = workgroup_id.x * 256u + local_id.x;
    if (tile_ix >= config.tile_count) {
        return;
    }
    let coarse_chunk_ix = tile_ix / 256u;
    store_tile_emit_chunk_offset(
        tile_ix,
        tile_emit_chunk_offset_at(tile_ix) + chunk_records[coarse_chunk_ix].ptcl_offset,
    );
}

@compute @workgroup_size(256)
fn coarse_emit_fill_refs(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = workgroup_id.x * 256u + local_id.x;
    if (tile_ix >= config.tile_count) {
        return;
    }
    let count = tile_emit_chunk_count_at(tile_ix);
    let offset = tile_emit_chunk_offset_at(tile_ix);
    var local_chunk = 0u;
    loop {
        if (local_chunk >= count) {
            break;
        }
        let ref_ix = offset + local_chunk;
        if (ref_ix < config.emit_chunk_capacity) {
            store_emit_chunk_ref(ref_ix, tile_ix, local_chunk);
        }
        local_chunk += 1u;
    }
}

@compute @workgroup_size(256)
fn coarse_emit_chunk_particle_counts(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let ref_ix = workgroup_id.x;
    let lane = local_id.x;
    let chunk = emit_chunk_at(ref_ix);
    let tile_ix = chunk.tile;
    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    let draw_ref_ix = tile_draw_start_at(tile_ix) + chunk.local_chunk * 256u + lane;
    var ptcl_count = 0u;
    var glyph_count = 0u;
    var class_flags = 0u;
    let wrapper_count = active_stack_count(tile_x, tile_y);

    if (wrapper_count != INVALID && draw_ref_ix < tile_draw_end_at(tile_ix)) {
        let draw_ix = tile_draw_index_at(draw_ref_ix);
        if (draw_in_batch(draw_ix)) {
            let draw_tag = draw_tag_at(draw_ix);
            var ptcl_tag = GPU_PTCL_FILL;
            if (draw_has_glyph_at(draw_ix)) {
                if (draw_tag == GPU_DRAW_BRUSH) {
                    glyph_count = count_tile_glyphs_for_run(draw_records[draw_ix].glyph_run_id, tile_x, tile_y);
                    ptcl_count = select(0u, 1u, glyph_count > 0u);
                    ptcl_tag = GPU_PTCL_GLYPH;
                }
            } else if (draw_has_sdf_at(draw_ix)) {
                if (draw_tag == GPU_DRAW_BRUSH) {
                    ptcl_count = 1u;
                    if (draw_sdf_full_tile_solid_color_at(draw_ix, tile_x, tile_y) != 0u) {
                        ptcl_tag = GPU_PTCL_COLOR;
                    } else if (draw_sdf_full_tile_image_at(draw_ix, tile_x, tile_y)) {
                        ptcl_tag = GPU_PTCL_IMAGE;
                    } else {
                        ptcl_tag = GPU_PTCL_SDF;
                    }
                }
            } else {
                let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
                if (backdrop_ix != INVALID) {
                    let segment_range = segment_ranges[backdrop_ix];
                    if (
                        (draw_tag == GPU_DRAW_BRUSH || draw_tag == GPU_DRAW_PATH_GLYPH || draw_tag == GPU_DRAW_CLIP) &&
                        (segment_range.start != segment_range.end || atomicLoad(&backdrops[backdrop_ix]) != 0i)
                    ) {
                        ptcl_count = 1u;
                        if (draw_tag == GPU_DRAW_CLIP) {
                            ptcl_tag = GPU_PTCL_BEGIN_CLIP;
                        } else if (draw_tag == GPU_DRAW_PATH_GLYPH) {
                            ptcl_tag = GPU_PTCL_PATH_GLYPH;
                        } else if (draw_solid_color_fast_path_at(draw_ix) && segment_range.start == segment_range.end) {
                            ptcl_tag = GPU_PTCL_COLOR;
                        }
                    }
                }
            }
            if (ptcl_count != 0u) {
                class_flags = particle_class_flags(ptcl_tag, draw_ix);
            }
        }
    }

    let chunk_ptcl_count = workgroup_sum(ptcl_count, lane);
    let chunk_glyph_count = workgroup_sum(glyph_count, lane);
    let color_count = workgroup_sum(select(0u, 1u, (class_flags & EMIT_CHUNK_CLASS_COLOR) != 0u), lane);
    let sdf_count = workgroup_sum(select(0u, 1u, (class_flags & EMIT_CHUNK_CLASS_SDF) != 0u), lane);
    let other_count = workgroup_sum(select(0u, 1u, (class_flags & EMIT_CHUNK_CLASS_OTHER) != 0u), lane);
    if (lane == 0u) {
        store_emit_chunk_counts(ref_ix, chunk_ptcl_count, chunk_glyph_count);
        store_emit_chunk_class_flags(
            ref_ix,
            select(0u, EMIT_CHUNK_CLASS_COLOR, color_count != 0u) |
                select(0u, EMIT_CHUNK_CLASS_SDF, sdf_count != 0u) |
                select(0u, EMIT_CHUNK_CLASS_OTHER, other_count != 0u),
        );
    }
}

@compute @workgroup_size(256)
fn coarse_emit_chunk_particle_offsets(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = workgroup_id.x * 256u + local_id.x;
    if (tile_ix >= config.tile_count) {
        return;
    }
    let chunk_count = tile_emit_chunk_count_at(tile_ix);
    let chunk_offset = tile_emit_chunk_offset_at(tile_ix);
    var local_chunk = 0u;
    var ptcl_offset = 0u;
    var glyph_offset = 0u;
    loop {
        if (local_chunk >= chunk_count) {
            break;
        }
        let ref_ix = chunk_offset + local_chunk;
        let chunk = emit_chunk_at(ref_ix);
        store_emit_chunk_offsets(ref_ix, ptcl_offset, glyph_offset);
        ptcl_offset += chunk.ptcl_count;
        glyph_offset += chunk.glyph_count;
        local_chunk += 1u;
    }
}

@compute @workgroup_size(256)
fn coarse_tile_counts_from_emit_chunks(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let tile_ix = workgroup_id.x * 256u + local_id.x;
    if (tile_ix >= config.tile_count) {
        return;
    }
    let chunk_count = tile_emit_chunk_count_at(tile_ix);
    let chunk_offset = tile_emit_chunk_offset_at(tile_ix);
    var local_chunk = 0u;
    var ptcl_count = 0u;
    var glyph_count = 0u;
    var flags = 0u;
    loop {
        if (local_chunk >= chunk_count) {
            break;
        }
        let chunk = emit_chunk_at(chunk_offset + local_chunk);
        ptcl_count += chunk.ptcl_count;
        glyph_count += chunk.glyph_count;
        flags |= chunk.class_flags;
        local_chunk += 1u;
    }

    if (ptcl_count > 0u) {
        let tile_x = tile_ix % config.tiles_width;
        let tile_y = tile_ix / config.tiles_width;
        let wrapper_count = active_stack_count(tile_x, tile_y);
        if (wrapper_count != INVALID) {
            let analytic_stack = wrapper_count != 0u && active_stack_is_analytic_clip_only(tile_x, tile_y);
            flags |= select(0u, EMIT_CHUNK_CLASS_OTHER, wrapper_count != 0u && !analytic_stack) |
                select(0u, EMIT_CHUNK_CLASS_STACK, analytic_stack);
            ptcl_count += wrapper_count * 2u + 1u;
        } else {
            ptcl_count = 0u;
            glyph_count = 0u;
            flags = EMIT_CHUNK_CLASS_OTHER;
        }
    }
    coarse_store_tile_counts(tile_ix, ptcl_count, glyph_count);
    store_fine_tile_kind(tile_ix, classify_fine_tile_kind_from_flags(flags));
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
                    if (layer_tag == GPU_LAYER_CLIP && draw_sdf_clip_fully_covers_tile_at(draw_ix, tile_x, tile_y)) {
                    } else if (draw_tile_hit(draw_ix, tile_x, tile_y)) {
                        count += 1u;
                    } else {
                        valid = false;
                    }
                } else {
                    let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
                    if (backdrop_ix == INVALID) {
                        valid = false;
                    } else if (
                        layer_tag == GPU_LAYER_CLIP &&
                        path_backdrop_fully_covers_tile(backdrop_ix, draw_records[draw_ix].fill_rule)
                    ) {
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

fn active_stack_is_analytic_clip_only(tile_x: u32, tile_y: u32) -> bool {
    var supported = true;
    var stack_ix = config.layer_stack_start;
    loop {
        if (stack_ix >= config.layer_stack_end) {
            break;
        }
        if (supported) {
            let layer = layer_stack[stack_ix];
            if (!active_stack_layer_is_noop_clip(layer, tile_x, tile_y)) {
                supported = layer.tag == GPU_LAYER_CLIP;
            }
        }
        stack_ix += 1u;
    }
    return supported;
}

fn active_stack_layer_is_noop_clip(layer: LayerStackRecord, tile_x: u32, tile_y: u32) -> bool {
    if (layer.tag != GPU_LAYER_CLIP) {
        return false;
    }
    let draw_ix = layer.draw;
    if (draw_has_sdf_at(draw_ix)) {
        return draw_sdf_clip_fully_covers_tile_at(draw_ix, tile_x, tile_y);
    }
    let backdrop_ix = draw_backdrop_ix(draw_ix, tile_x, tile_y);
    return backdrop_ix != INVALID &&
        path_backdrop_fully_covers_tile(backdrop_ix, draw_records[draw_ix].fill_rule);
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
    let run = text_run_at(run_id);
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

fn glyph_hits_tile(glyph_ix: u32, tile_x: u32, tile_y: u32) -> bool {
    let glyph = glyph_at(glyph_ix);
    let image_id = glyph.image_id;
    if (image_id == INVALID) {
        return false;
    }
    let image = glyph_image_at(image_id);
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

fn draw_in_batch(draw_ix: u32) -> bool {
    return draw_ix >= config.draw_start && draw_ix < config.draw_end;
}

fn draw_tag_at(draw_ix: u32) -> u32 {
    return draw_records[draw_ix].tag;
}

fn draw_has_sdf_at(draw_ix: u32) -> bool {
    let draw = draw_records[draw_ix];
    return draw.sdf_offset != INVALID || draw.sdf_shadow_offset != INVALID;
}

fn draw_has_glyph_at(draw_ix: u32) -> bool {
    return config.text_enabled != 0u && draw_records[draw_ix].glyph_run_id != INVALID;
}
