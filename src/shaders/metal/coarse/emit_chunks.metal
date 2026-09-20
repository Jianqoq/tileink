#include <metal_stdlib>
using namespace metal;
#include "config.metal"
#include "classify.metal"
#include "draw_list.metal"
#include "prefix_scan.metal"
#include "emit_draw.metal"
#include "emit_stack.metal"

kernel void coarse_emit_chunks(constant CoarseConfig& c [[buffer(0)]],
    const device uint* raw_draws [[buffer(1)]], const device uint* raw_text [[buffer(2)]], const device uint* raw_paint [[buffer(3)]],
    const device uint* raw_paths [[buffer(4)]], const device uint* raw_backdrops [[buffer(5)]],
    const device packed_uint2* ranges [[buffer(6)]], const device uint* raw_layers [[buffer(7)]],
    device uint* work [[buffer(8)]], const device uint* raw_batches [[buffer(9)]], constant BufferSizes& sizes [[buffer(29)]], constant uint4& grid [[buffer(30)]],
    uint3 group [[threadgroup_position_in_grid]], uint lane [[thread_index_in_threadgroup]]) {
    Words draws{raw_draws, word_size(sizes, 1)};
    Words text{raw_text, word_size(sizes, 2)};
    Words paint{raw_paint, word_size(sizes, 3)};
    Words paths{raw_paths, word_size(sizes, 4)};
    Words backdrops{raw_backdrops, word_size(sizes, 5)};
    Words layers{raw_layers, word_size(sizes, 7)};
    Words batches{raw_batches, word_size(sizes, 9)};

    if (!c.tile_count) return;
    uint reference = group.x + grid.x * (group.y + grid.y * group.z);
    uint last = tile_emit_base(c, c.tile_count - 1);
    if (reference >= c.emit_chunk_capacity || reference >= work[last] + work[last + 1]) return;
    uint base = emit_base(c, reference), tile = work[base], chunk = work[base + 1];
    uint2 position(tile % c.tiles_width, tile / c.tiles_width);
    uint cursor = work[tile * 6 + 1], end = work[tile * 6 + 2];
    uint glyph = work[tile * 6 + 4] + work[base + 5], glyph_end = work[tile * 6 + 5];
    uint wrappers = wrapper_count(c, layers, draws, paths, backdrops, ranges, paint, position);
    bool emit_wrappers = cursor < end && wrappers != invalid_index;
    if (!lane && emit_wrappers && !chunk) emit_stack(c, work, layers, draws, paths, backdrops, ranges, paint, cursor, position, false);
    cursor += (emit_wrappers ? wrappers : 0) + work[base + 3];
    uint page = page_at(work, c, tile, chunk), ordinal = chunk * 256 + lane;
    uint index = page != invalid_index && ordinal < work[tile_draw_base(c, tile) + 1] ? page_index(work, c, page, lane) : invalid_index;
    Particle p = empty_particle();
    if (emit_wrappers) p = draw_particle(c, draws, text, paint, paths, backdrops, ranges, batches, index, position);
    threadgroup uint2 scratch[256];
    uint2 total;
    uint2 offset = exclusive_prefix(uint2(p.valid, p.glyph_count), lane, scratch, total);
    uint flags = 0;
    if (p.valid) {
        DrawData d = load_draw(draws, index);
        flags = particle_flags(paint, c, d, p.tag);
        if (p.tag == 10) {
            p.segments = uint2(glyph + offset.y, glyph + offset.y + p.glyph_count);
            if (p.segments.y <= glyph_end) store_glyphs(work, text, c, d, position, p.segments.x);
        }
        store_particle(work, c, cursor + offset.x, p);
    }
    if (!lane && emit_wrappers && chunk + 1 == work[tile_emit_base(c, tile)]) {
        uint end_cursor = cursor + work[base + 2];
        emit_stack(c, work, layers, draws, paths, backdrops, ranges, paint, end_cursor, position, true);
        store_particle(work, c, end_cursor + wrappers, 0, 0, 0, uint2(0), 0);
    }
    threadgroup uint classification[256];
    classification[lane] = flags;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for (uint step = 128; step; step /= 2) {
        if (lane < step) classification[lane] |= classification[lane + step];
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if (!lane) work[base + 6] = classification[0];
}
