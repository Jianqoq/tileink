#include "common.wgsl"
#include "text_input.wgsl"

@group(0) @binding(1) var<storage, read> draw_records: array<DrawRecord>;
@group(0) @binding(2) var<storage, read> text_blob: array<u32>;
@group(0) @binding(3) var<storage, read> path_records: array<PathRecord>;
@group(0) @binding(4) var<storage, read_write> backdrops: array<atomic<i32>>;
@group(0) @binding(5) var<storage, read> segment_ranges: array<TileSegmentRange>;
@group(0) @binding(6) var<storage, read> layer_stack: array<LayerStackRecord>;
@group(0) @binding(7) var<storage, read_write> coarse_work: array<u32>;

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
        coarse_store_tile_counts(tile_ix, stored_count, tile_glyph_count);
        store_fine_tile_kind(
            tile_ix,
            select(FINE_TILE_KIND_EMPTY_OR_CLEAR, FINE_TILE_KIND_FULL_INTERPRETER, stored_count > 0u),
        );
    }
}

@compute @workgroup_size(256)
fn coarse_count_bins(
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

    let wrapper_count = active_stack_count(tile_x, tile_y);
    var count = 0u;
    var glyph_count = 0u;
    if (wrapper_count != INVALID) {
        var draw_ref_ix = tile_draw_start_at(tile_ix);
        let tile_draw_end = tile_draw_end_at(tile_ix);
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
                        let segment_range = segment_ranges[backdrop_ix];
                        if (
                            (draw_tag == GPU_DRAW_BRUSH || draw_tag == GPU_DRAW_PATH_GLYPH || draw_tag == GPU_DRAW_CLIP) &&
                            (segment_range.start != segment_range.end || atomicLoad(&backdrops[backdrop_ix]) != 0i)
                        ) {
                            count += 1u;
                        }
                    }
                }
            }
            draw_ref_ix += 1u;
        }
    }

    var stored_count = count;
    if (count > 0u) {
        stored_count = count + wrapper_count * 2u + 1u;
    }
    coarse_store_tile_counts(tile_ix, stored_count, glyph_count);
    store_fine_tile_kind(
        tile_ix,
        select(FINE_TILE_KIND_EMPTY_OR_CLEAR, FINE_TILE_KIND_FULL_INTERPRETER, stored_count > 0u),
    );
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
    return draw_records[draw_ix].glyph_run_id != INVALID;
}
