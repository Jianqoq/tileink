#include <metal_stdlib>
using namespace metal;
#include "config.metal"
#include "classify.metal"
#include "draw_list.metal"
kernel void coarse_tile_counts_from_emit_chunks(constant CoarseConfig& c [[buffer(0)]],
    const device uint* raw_draws [[buffer(1)]], const device uint* raw_paths [[buffer(3)]],
    const device uint* raw_backdrops [[buffer(4)]], const device packed_uint2* ranges [[buffer(5)]],
    const device uint* raw_layers [[buffer(6)]], device uint* work [[buffer(7)]], const device uint* raw_sdf [[buffer(9)]],
    constant BufferSizes& sizes [[buffer(29)]], uint tile [[thread_position_in_grid]]) {
    Words draws{raw_draws, word_size(sizes, 1)};
    Words paths{raw_paths, word_size(sizes, 3)};
    Words backdrops{raw_backdrops, word_size(sizes, 4)};
    Words layers{raw_layers, word_size(sizes, 6)};
    Words sdf{raw_sdf, word_size(sizes, 9)};

    if (tile >= c.tile_count) return;
    uint range = tile_emit_base(c, tile);
    uint2 count(0);
    for (uint i = 0; i < work[range]; ++i) {
        uint base = emit_base(c, work[range + 1] + i);
        count += uint2(work[base + 2], work[base + 4]);
    }
    if (count.x) {
        uint2 position(tile % c.tiles_width, tile / c.tiles_width);
        uint wrappers = wrapper_count(c, layers, draws, paths, backdrops, ranges, sdf, position);
        if (wrappers == invalid_index) count = uint2(0);
        else count.x += wrappers * 2 + 1;
    }
    store_counts(work, c, tile, count);
}
