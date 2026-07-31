#include "common.wgsl"
#include "text_input.wgsl"

@group(0) @binding(1) var<storage, read> draw_records: array<DrawRecord>;
@group(0) @binding(2) var<storage, read> text_blob: array<u32>;
@group(0) @binding(3) var<storage, read> sdf_blob: array<u32>;
@group(0) @binding(4) var<storage, read> path_records: array<PathRecord>;
@group(0) @binding(5) var<storage, read_write> backdrops: array<atomic<i32>>;
@group(0) @binding(6) var<storage, read> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(7) var<storage, read> layer_stack: array<LayerStackRecord>;
@group(0) @binding(8) var<storage, read_write> coarse_work: array<u32>;
@group(0) @binding(9) var<storage, read> draw_batch_ids: array<u32>;

@compute @workgroup_size(256)
fn coarse_emit(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    if (workgroup_id.x >= config.active_tile_count) {
        return;
    }
    let tile_ix = dispatched_tile_at(workgroup_id.x);
    if (tile_ix >= config.tile_count) {
        return;
    }

    let lane = local_id.x;
    let tile_x = tile_ix % config.tiles_width;
    let tile_y = tile_ix / config.tiles_width;
    let tile = coarse_load_tile(tile_ix);
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

    var page = tile_draw_head_at(tile_ix);
    var remaining = tile_draw_count_at(tile_ix);
    loop {
        if (page == INVALID || remaining == 0u) {
            break;
        }

        let page_count = min(remaining, TILE_DRAW_PAGE_SIZE);
        var draw_ix = INVALID;
        var valid = false;
        var glyph_count = 0u;
        var ptcl_tag = GPU_PTCL_FILL;
        var ptcl_backdrop = 0i;
        var ptcl_fill_rule = 0u;
        var ptcl_segment_start = 0u;
        var ptcl_segment_end = 0u;
        var ptcl_color = 0u;

        if (lane < page_count) {
            draw_ix = tile_draw_index_in_page(page, lane);
        }

        if (draw_in_batch(draw_ix)) {
            let draw_tag = draw_tag_at(draw_ix);
            if (draw_has_glyph_at(draw_ix)) {
                if (draw_tag == GPU_DRAW_BRUSH) {
                    glyph_count = count_tile_glyphs_for_run(draw_ix, tile_x, tile_y);
                    if (glyph_count > 0u) {
                        valid = true;
                        ptcl_tag = GPU_PTCL_GLYPH;
                        ptcl_color = draw_ix;
                    }
                }
            } else if (draw_has_sdf_at(draw_ix)) {
                if (draw_tag == GPU_DRAW_BRUSH) {
                    valid = true;
                    let solid_color = draw_sdf_full_tile_solid_color_at(draw_ix, tile_x, tile_y);
                    if (solid_color != 0u) {
                        ptcl_tag = GPU_PTCL_COLOR;
                        ptcl_color = solid_color;
                    } else if (draw_sdf_full_tile_image_at(draw_ix, tile_x, tile_y)) {
                        ptcl_tag = GPU_PTCL_IMAGE;
                        ptcl_color = draw_ix;
                    } else {
                        ptcl_tag = GPU_PTCL_SDF;
                        ptcl_segment_start = draw_ix;
                        ptcl_color = draw_ix;
                    }
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
                    store_tile_glyphs_for_run(ptcl_segment_start, draw_ix, tile_x, tile_y);
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
        remaining -= page_count;
        page = tile_draw_next_page(page);
    }

    if (lane == 0u) {
        emit_active_stack_ends(cursor, tile_x, tile_y);
        store_particle(cursor + wrapper_count, GPU_PTCL_END, 0i, 0u, 0u, 0u, 0u);
    }
}

@compute @workgroup_size(256)
fn coarse_emit_bins(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let lanes_per_bin_x = 16u;
    let bins_per_row = (config.tiles_width + lanes_per_bin_x - 1u) / lanes_per_bin_x;
    let bin_x = workgroup_id.x % bins_per_row;
    let bin_y = workgroup_id.x / bins_per_row;
    let lane = local_id.x;
    let tile_x = bin_x * lanes_per_bin_x + lane % lanes_per_bin_x;
    let tile_y = bin_y * lanes_per_bin_x + lane / lanes_per_bin_x;
    if (tile_x >= config.tiles_width || tile_y >= config.tiles_height) {
        return;
    }

    let tile_ix = tile_y * config.tiles_width + tile_x;
    if (tile_ix >= config.tile_count) {
        return;
    }

    let tile = coarse_load_tile(tile_ix);
    var cursor = tile.ptcl_start;
    let range_end = tile.ptcl_end;
    var glyph_cursor = tile.glyph_start;
    let glyph_range_end = tile.glyph_end;
    if (cursor >= range_end) {
        store_fine_tile_kind(tile_ix, FINE_TILE_KIND_EMPTY_OR_CLEAR);
        return;
    }

    let wrapper_count = active_stack_count(tile_x, tile_y);
    if (wrapper_count == INVALID) {
        store_fine_tile_kind(tile_ix, FINE_TILE_KIND_FULL_INTERPRETER);
        return;
    }

    var saw_color = false;
    var saw_sdf = false;
    var saw_other = wrapper_count != 0u;

    emit_active_stack_begins(cursor, tile_x, tile_y);
    cursor += wrapper_count;

    var page = tile_draw_head_at(tile_ix);
    var remaining = tile_draw_count_at(tile_ix);
    var slot = 0u;
    loop {
        if (page == INVALID || remaining == 0u) {
            break;
        }

        let draw_ix = tile_draw_index_in_page(page, slot);
        var valid = false;
        var glyph_count = 0u;
        var ptcl_tag = GPU_PTCL_FILL;
        var ptcl_backdrop = 0i;
        var ptcl_fill_rule = 0u;
        var ptcl_segment_start = 0u;
        var ptcl_segment_end = 0u;
        var ptcl_color = 0u;

        if (draw_in_batch(draw_ix)) {
            let draw_tag = draw_tag_at(draw_ix);
            if (draw_has_glyph_at(draw_ix)) {
                if (draw_tag == GPU_DRAW_BRUSH) {
                    glyph_count = count_tile_glyphs_for_run(draw_ix, tile_x, tile_y);
                    if (glyph_count > 0u) {
                        valid = true;
                        ptcl_tag = GPU_PTCL_GLYPH;
                        ptcl_color = draw_ix;
                    }
                }
            } else if (draw_has_sdf_at(draw_ix)) {
                if (draw_tag == GPU_DRAW_BRUSH) {
                    valid = true;
                    let solid_color = draw_sdf_full_tile_solid_color_at(draw_ix, tile_x, tile_y);
                    if (solid_color != 0u) {
                        ptcl_tag = GPU_PTCL_COLOR;
                        ptcl_color = solid_color;
                    } else if (draw_sdf_full_tile_image_at(draw_ix, tile_x, tile_y)) {
                        ptcl_tag = GPU_PTCL_IMAGE;
                        ptcl_color = draw_ix;
                    } else {
                        ptcl_tag = GPU_PTCL_SDF;
                        ptcl_segment_start = draw_ix;
                        ptcl_color = draw_ix;
                    }
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

        if (valid) {
            if (ptcl_tag == GPU_PTCL_COLOR) {
                saw_color = true;
            } else if (ptcl_tag == GPU_PTCL_IMAGE) {
                saw_sdf = true;
            } else if (ptcl_tag == GPU_PTCL_SDF && draw_solid_supported_sdf_at(draw_ix)) {
                saw_sdf = true;
            } else {
                saw_other = true;
            }
            if (ptcl_tag == GPU_PTCL_GLYPH) {
                ptcl_segment_start = glyph_cursor;
                ptcl_segment_end = ptcl_segment_start + glyph_count;
                if (ptcl_segment_end <= glyph_range_end) {
                    store_tile_glyphs_for_run(ptcl_segment_start, draw_ix, tile_x, tile_y);
                }
                glyph_cursor = ptcl_segment_end;
            }
            if (cursor < range_end) {
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
        }
        slot += 1u;
        remaining -= 1u;
        if (slot == TILE_DRAW_PAGE_SIZE) {
            page = tile_draw_next_page(page);
            slot = 0u;
        }
    }

    emit_active_stack_ends(cursor, tile_x, tile_y);
    store_particle(cursor + wrapper_count, GPU_PTCL_END, 0i, 0u, 0u, 0u, 0u);
    store_fine_tile_kind(tile_ix, classify_fine_tile_kind(saw_color, saw_sdf, saw_other));
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
            } else if (active_stack_layer_is_noop_clip(layer, tile_x, tile_y)) {
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
        if (
            (layer_tag == GPU_LAYER_CLIP || layer_tag == GPU_LAYER_OPACITY || layer_tag == GPU_LAYER_BLEND) &&
            !active_stack_layer_is_noop_clip(layer, tile_x, tile_y)
        ) {
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

fn emit_active_stack_ends(dst_start: u32, tile_x: u32, tile_y: u32) {
    var stack_ix = config.layer_stack_end;
    var dst = dst_start;
    loop {
        if (stack_ix <= config.layer_stack_start) {
            break;
        }
        stack_ix -= 1u;
        let layer = layer_stack[stack_ix];
        let layer_tag = layer.tag;
        var ptcl_tag = GPU_PTCL_END_CLIP;
        var valid = true;
        if (layer_tag == GPU_LAYER_OPACITY) {
            ptcl_tag = GPU_PTCL_END_OPACITY;
        } else if (layer_tag == GPU_LAYER_BLEND) {
            ptcl_tag = GPU_PTCL_END_BLEND;
        } else if (layer_tag != GPU_LAYER_CLIP) {
            valid = false;
        }
        if (active_stack_layer_is_noop_clip(layer, tile_x, tile_y)) {
            valid = false;
        }
        if (valid) {
            store_particle(dst, ptcl_tag, 0i, 0u, 0u, 0u, 0u);
            dst += 1u;
        }
    }
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
        path_backdrop_fully_covers_tile(backdrop_ix, draw_fill_rule_at(draw_ix));
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

fn count_tile_glyphs_for_run(draw_ix: u32, tile_x: u32, tile_y: u32) -> u32 {
    var count = 0u;
    let draw = draw_records[draw_ix];
    let run_id = draw.glyph_run_id;
    let run = text_run_at(run_id);
    var glyph_ix = run.glyph_start;
    let glyph_end = glyph_ix + run.glyph_count;
    loop {
        if (glyph_ix >= glyph_end) {
            break;
        }
        if (glyph_hits_tile(draw, glyph_ix, tile_x, tile_y)) {
            count += 1u;
        }
        glyph_ix += 1u;
    }
    return count;
}

fn store_tile_glyphs_for_run(dst_start: u32, draw_ix: u32, tile_x: u32, tile_y: u32) {
    var count = 0u;
    let draw = draw_records[draw_ix];
    let run_id = draw.glyph_run_id;
    let run = text_run_at(run_id);
    var glyph_ix = run.glyph_start;
    let glyph_end = glyph_ix + run.glyph_count;
    loop {
        if (glyph_ix >= glyph_end) {
            break;
        }
        if (glyph_hits_tile(draw, glyph_ix, tile_x, tile_y)) {
            let dst = dst_start + count;
            if (dst < config.glyph_capacity) {
                coarse_store_glyph(dst, glyph_ix);
            }
            count += 1u;
        }
        glyph_ix += 1u;
    }
}

fn glyph_hits_tile(draw: DrawRecord, glyph_ix: u32, tile_x: u32, tile_y: u32) -> bool {
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
    return transformed_rect_hits_tile(draw.transform, x0, y0, x1, y1, tile_x, tile_y);
}

fn draw_in_batch(draw_ix: u32) -> bool {
    return draw_ix < arrayLength(&draw_batch_ids) && draw_batch_ids[draw_ix] == config.draw_start;
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
    return config.text_enabled != 0u && draw_records[draw_ix].glyph_run_id != INVALID;
}

fn draw_solid_color_fast_path_at(draw_ix: u32) -> bool {
    let draw = draw_records[draw_ix];
    // The axis-aligned bounds are the exact rectangle only when the node transform has no
    // rotation or shear. Other affine rectangles must go through SDF coverage in fine.
    return draw.solid_rect != 0u && draw.transform.b == 0.0 && draw.transform.c == 0.0
        && draw_has_nontransparent_solid_brush_at(draw_ix);
}

fn classify_fine_tile_kind(saw_color: bool, saw_sdf: bool, saw_other: bool) -> u32 {
    if (saw_other) {
        return FINE_TILE_KIND_FULL_INTERPRETER;
    }
    if (saw_sdf && saw_color) {
        return FINE_TILE_KIND_MIXED_ANALYTIC_SOLID_NO_STACK;
    }
    if (saw_sdf) {
        return FINE_TILE_KIND_PURE_SDF_SOLID_NO_STACK;
    }
    if (saw_color) {
        return FINE_TILE_KIND_COLOR_ONLY_NO_STACK;
    }
    return FINE_TILE_KIND_EMPTY_OR_CLEAR;
}

fn draw_solid_supported_sdf_at(draw_ix: u32) -> bool {
    let draw = draw_records[draw_ix];
    if (
        !draw_has_nontransparent_solid_brush_at(draw_ix) ||
        draw.sdf_offset == INVALID ||
        draw.sdf_shadow_offset != INVALID ||
        draw.sdf_len == 0u
    ) {
        return false;
    }
    let kind = sdf_blob[draw.sdf_offset];
    return kind == GPU_SDF_RECT || kind == GPU_SDF_CANDLESTICK || kind == GPU_SDF_CHECKERBOARD;
}

fn draw_has_nontransparent_solid_brush_at(draw_ix: u32) -> bool {
    let brush_offset = draw_records[draw_ix].brush_offset;
    if (brush_offset == INVALID) {
        return false;
    }
    let brush_base = config.paint_brush_base + brush_offset;
    return sdf_blob[brush_base] == GPU_BRUSH_SOLID &&
        sdf_blob[brush_base + 4u] != 0u;
}

fn draw_solid_color_at(draw_ix: u32) -> u32 {
    return sdf_blob[config.paint_brush_base + draw_records[draw_ix].brush_offset + 4u];
}

fn draw_sdf_full_tile_solid_color_at(draw_ix: u32, tile_x: u32, tile_y: u32) -> u32 {
    let draw = draw_records[draw_ix];
    var color = 0u;
    if (
        draw_has_nontransparent_solid_brush_at(draw_ix) &&
        draw.sdf_offset != INVALID &&
        draw.sdf_shadow_offset == INVALID &&
        draw.sdf_len >= 9u &&
        sdf_blob[draw.sdf_offset] == GPU_SDF_RECT &&
        sdf_rect_fully_covers_tile(draw, tile_x, tile_y)
    ) {
        color = draw_solid_color_at(draw_ix);
    }
    return color;
}

fn draw_sdf_full_tile_image_at(draw_ix: u32, tile_x: u32, tile_y: u32) -> bool {
    let draw = draw_records[draw_ix];
    return draw_has_opaque_image_brush_at(draw_ix) &&
        draw.sdf_offset != INVALID &&
        draw.sdf_shadow_offset == INVALID &&
        draw.sdf_len >= 9u &&
        sdf_blob[draw.sdf_offset] == GPU_SDF_RECT &&
        sdf_rect_fully_covers_tile(draw, tile_x, tile_y);
}

fn draw_has_opaque_image_brush_at(draw_ix: u32) -> bool {
    let brush_offset = draw_records[draw_ix].brush_offset;
    if (brush_offset == INVALID) {
        return false;
    }
    let brush_base = config.paint_brush_base + brush_offset;
    return sdf_blob[brush_base] == GPU_BRUSH_PATTERN_RESOURCE &&
        sdf_blob[brush_base + 7u] == 255u;
}

fn store_particle(dst: u32, tag: u32, backdrop: i32, fill_rule: u32, segment_start: u32, segment_end: u32, color: u32) {
    if (dst < config.ptcl_capacity) {
        coarse_store_ptcl(dst, tag, backdrop, fill_rule, segment_start, segment_end, color);
    }
}
