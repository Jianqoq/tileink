#include <metal_stdlib>
using namespace metal;
#include "config.metal"
#include "classify.metal"
#include "tile_classification.metal"
kernel void coarse_emit_chunk_tile_kinds(constant CoarseConfig& c [[buffer(0)]],
    const device uint* raw_draws [[buffer(1)]], const device uint* raw_sdf [[buffer(3)]],
    const device uint* raw_paths [[buffer(4)]], const device uint* raw_backdrops [[buffer(5)]],
    const device packed_uint2* ranges [[buffer(6)]], const device uint* raw_layers [[buffer(7)]],
    device uint* work [[buffer(8)]], constant BufferSizes& sizes [[buffer(29)]], uint tile [[thread_position_in_grid]]) {
    Words draws{raw_draws, word_size(sizes, 1)};
    Words sdf{raw_sdf, word_size(sizes, 3)};
    Words paths{raw_paths, word_size(sizes, 4)};
    Words backdrops{raw_backdrops, word_size(sizes, 5)};
    Words layers{raw_layers, word_size(sizes, 7)};

    if (tile >= c.tile_count) return;
    uint kind = 1;
    if (work[tile * 6]) {
        uint2 position(tile % c.tiles_width, tile / c.tiles_width);
        uint wrappers = wrapper_count(c, layers, draws, paths, backdrops, ranges, sdf, position);
        uint flags = wrappers ? 4 : 0;
        if (wrappers != invalid_index) {
            uint range = tile_emit_base(c, tile);
            for (uint i = 0; i < work[range]; ++i) flags |= work[emit_base(c, work[range + 1] + i) + 6];
        }
        kind = tile_kind(flags);
    }
    work[emit_base(c, c.emit_chunk_capacity) + tile] = kind;
}
