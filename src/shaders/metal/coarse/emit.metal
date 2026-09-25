#include <metal_stdlib>
using namespace metal;
#include "config.metal"
#include "classify.metal"
#include "draw_list.metal"
#include "prefix_scan.metal"
#include "emit_draw.metal"
#include "emit_stack.metal"

kernel void coarse_emit(constant CoarseConfig& c [[buffer(0)]],
    const device uint* raw_draws [[buffer(1)]], const device uint* raw_text [[buffer(2)]], const device uint* raw_paint [[buffer(3)]],
    const device uint* raw_paths [[buffer(4)]], const device uint* raw_backdrops [[buffer(5)]],
    const device packed_uint2* ranges [[buffer(6)]], const device uint* raw_layers [[buffer(7)]],
    device uint* work [[buffer(8)]], const device uint* raw_batches [[buffer(9)]],
    constant BufferSizes& sizes [[buffer(29)]], uint group [[threadgroup_position_in_grid]], uint lane [[thread_index_in_threadgroup]]) {
    Words draws{raw_draws, word_size(sizes, 1)};
    Words text{raw_text, word_size(sizes, 2)};
    Words paint{raw_paint, word_size(sizes, 3)};
    Words paths{raw_paths, word_size(sizes, 4)};
    Words backdrops{raw_backdrops, word_size(sizes, 5)};
    Words layers{raw_layers, word_size(sizes, 7)};
    Words batches{raw_batches, word_size(sizes, 9)};

    if (group >= c.active_tile_count) return;
    uint tile = tile_at(work, c, group);
    if (tile >= c.tile_count) return;
    uint2 position(tile % c.tiles_width, tile / c.tiles_width);
    uint cursor = work[tile * 6 + 1], end = work[tile * 6 + 2];
    uint glyph = work[tile * 6 + 4], glyph_end = work[tile * 6 + 5];
    // Reused slots may still carry EMPTY/COLOR from a preceding scalar batch.
    // Parallel emission produces an interpreter stream, including a fresh terminator
    // when a clip rejects this tile; it must never expose the previous batch.
    if (!lane) work[emit_base(c, c.emit_chunk_capacity) + tile] = cursor < end ? 0 : 1;
    if (cursor >= end) return;
    uint wrappers = wrapper_count(c, layers, draws, paths, backdrops, ranges, paint, position);
    if (wrappers == invalid_index) {
        if (!lane) store_particle(work, c, cursor, 0, 0, 0, uint2(0), 0);
        return;
    }
    if (!lane) emit_stack(c, work, layers, draws, paths, backdrops, ranges, paint, cursor, position, false);
    cursor += wrappers;
    uint base = tile_draw_base(c, tile), page = work[base], remaining = work[base + 1];
    threadgroup uint2 scratch[256];
    while (page != invalid_index && remaining) {
        uint count = min(remaining, 256u);
        uint index = lane < count ? page_index(work, c, page, lane) : invalid_index;
        Particle p = draw_particle(c, draws, text, paint, paths, backdrops, ranges, batches, index, position);
        uint2 total;
        uint2 offset = exclusive_prefix(uint2(p.valid, p.glyph_count), lane, scratch, total);
        if (p.valid) {
            if (p.tag == 10) {
                p.segments = uint2(glyph + offset.y, glyph + offset.y + p.glyph_count);
                if (p.segments.y <= glyph_end) store_glyphs(work, text, c, load_draw(draws, index), position, p.segments.x);
            }
            store_particle(work, c, cursor + offset.x, p);
        }
        cursor += total.x; glyph += total.y;
        remaining -= count; page = page_next(work, c, page);
    }
    if (!lane) {
        emit_stack(c, work, layers, draws, paths, backdrops, ranges, paint, cursor, position, true);
        store_particle(work, c, cursor + wrappers, 0, 0, 0, uint2(0), 0);
    }
}
kernel void coarse_emit_bins(constant CoarseConfig& c [[buffer(0)]],
    const device uint* raw_draws [[buffer(1)]], const device uint* raw_text [[buffer(2)]], const device uint* raw_paint [[buffer(3)]],
    const device uint* raw_paths [[buffer(4)]], const device uint* raw_backdrops [[buffer(5)]],
    const device packed_uint2* ranges [[buffer(6)]], const device uint* raw_layers [[buffer(7)]],
    device uint* work [[buffer(8)]], const device uint* raw_batches [[buffer(9)]],
    constant BufferSizes& sizes [[buffer(29)]], uint group [[threadgroup_position_in_grid]], uint lane [[thread_index_in_threadgroup]]) {
    Words draws{raw_draws, word_size(sizes, 1)};
    Words text{raw_text, word_size(sizes, 2)};
    Words paint{raw_paint, word_size(sizes, 3)};
    Words paths{raw_paths, word_size(sizes, 4)};
    Words backdrops{raw_backdrops, word_size(sizes, 5)};
    Words layers{raw_layers, word_size(sizes, 7)};
    Words batches{raw_batches, word_size(sizes, 9)};

    uint tile;
    uint2 position;
    if (c.incremental) {
        // Scalar preallocated emission assigns one lane to each active tile.
        // Its list can be sparse/nonmonotonic, and the last group can be padded.
        uint active = group * 256 + lane;
        if (active >= c.active_tile_count) return;
        tile = tile_at(work, c, active);
        if (tile >= c.tile_count) return;
        position = uint2(tile % c.tiles_width, tile / c.tiles_width);
    } else {
        uint bins = (c.tiles_width + 15) / 16;
        position = uint2(group % bins, group / bins) * 16 + uint2(lane % 16, lane / 16);
        if (position.x >= c.tiles_width || position.y >= c.tiles_height) return;
        tile = position.y * c.tiles_width + position.x;
    }
    if (tile >= c.tile_count) return;
    uint kind = emit_base(c, c.emit_chunk_capacity) + tile;
    uint cursor = work[tile * 6 + 1], end = work[tile * 6 + 2];
    uint glyph = work[tile * 6 + 4], glyph_end = work[tile * 6 + 5];
    if (cursor >= end) { work[kind] = 1; return; }
    uint wrappers = wrapper_count(c, layers, draws, paths, backdrops, ranges, paint, position);
    if (wrappers == invalid_index) {
        store_particle(work, c, cursor, 0, 0, 0, uint2(0), 0);
        work[kind] = 1;
        return;
    }
    uint flags = wrappers ? 4 : 0;
    emit_stack(c, work, layers, draws, paths, backdrops, ranges, paint, cursor, position, false);
    cursor += wrappers;
    uint base = tile_draw_base(c, tile), page = work[base], remaining = work[base + 1], slot = 0;
    while (page != invalid_index && remaining) {
        uint index = page_index(work, c, page, slot);
        Particle p = draw_particle(c, draws, text, paint, paths, backdrops, ranges, batches, index, position);
        if (p.valid) {
            DrawData d = load_draw(draws, index);
            flags |= particle_flags(paint, c, d, p.tag);
            if (p.tag == 10) {
                p.segments = uint2(glyph, glyph + p.glyph_count);
                if (p.segments.y <= glyph_end) store_glyphs(work, text, c, d, position, glyph);
                glyph = p.segments.y;
            }
            if (cursor < end) store_particle(work, c, cursor++, p);
        }
        ++slot; --remaining;
        if (slot == 256) { page = page_next(work, c, page); slot = 0; }
    }
    emit_stack(c, work, layers, draws, paths, backdrops, ranges, paint, cursor, position, true);
    store_particle(work, c, cursor + wrappers, 0, 0, 0, uint2(0), 0);
    work[kind] = tile_kind(flags);
}
