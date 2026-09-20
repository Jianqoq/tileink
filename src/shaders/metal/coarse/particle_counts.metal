#include <metal_stdlib>
using namespace metal;
#include "config.metal"
#include "classify.metal"
#include "draw_list.metal"
#include "prefix_scan.metal"

kernel void coarse_emit_chunk_particle_counts(constant CoarseConfig& c [[buffer(0)]],
    const device uint* raw_draws [[buffer(1)]], const device uint* raw_text [[buffer(2)]],
    const device uint* raw_paths [[buffer(3)]], const device uint* raw_backdrops [[buffer(4)]],
    const device packed_uint2* ranges [[buffer(5)]], const device uint* raw_layers [[buffer(6)]],
    device uint* work [[buffer(7)]], const device uint* raw_sdf [[buffer(9)]], const device uint* raw_batches [[buffer(10)]],
    constant BufferSizes& sizes [[buffer(29)]], constant uint4& grid [[buffer(30)]], uint3 group [[threadgroup_position_in_grid]], uint lane [[thread_index_in_threadgroup]]) {
    Words draws{raw_draws, word_size(sizes, 1)};
    Words text{raw_text, word_size(sizes, 2)};
    Words paths{raw_paths, word_size(sizes, 3)};
    Words backdrops{raw_backdrops, word_size(sizes, 4)};
    Words layers{raw_layers, word_size(sizes, 6)};
    Words sdf{raw_sdf, word_size(sizes, 9)};
    Words batches{raw_batches, word_size(sizes, 10)};

    if (!c.tile_count) return;
    uint reference = group.x + grid.x * (group.y + grid.y * group.z);
    uint last = tile_emit_base(c, c.tile_count - 1);
    if (reference >= work[last] + work[last + 1]) return;
    uint base = emit_base(c, reference), tile = work[base], chunk = work[base + 1];
    uint2 position(tile % c.tiles_width, tile / c.tiles_width);
    uint wrappers = wrapper_count(c, layers, draws, paths, backdrops, ranges, sdf, position);
    uint page = page_at(work, c, tile, chunk), ordinal = chunk * 256 + lane;
    uint2 count(0), total;
    if (wrappers != invalid_index && page != invalid_index && ordinal < work[tile_draw_base(c, tile) + 1])
        count = particle_count(c, draws, text, paths, backdrops, ranges, batches, page_index(work, c, page, lane), position);
    threadgroup uint2 scratch[256];
    exclusive_prefix(count, lane, scratch, total);
    if (!lane) { work[base + 2] = total.x; work[base + 4] = total.y; }
}
