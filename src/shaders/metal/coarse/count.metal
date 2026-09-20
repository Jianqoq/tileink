#include <metal_stdlib>
using namespace metal;
#include "config.metal"
#include "classify.metal"
#include "draw_list.metal"
#include "prefix_scan.metal"

kernel void coarse_count(constant CoarseConfig& c [[buffer(0)]],
    const device uint* raw_draws [[buffer(1)]], const device uint* raw_text [[buffer(2)]],
    const device uint* raw_paths [[buffer(3)]], const device uint* raw_backdrops [[buffer(4)]],
    const device packed_uint2* ranges [[buffer(5)]], const device uint* raw_layers [[buffer(6)]],
    device uint* work [[buffer(7)]], const device uint* raw_sdf [[buffer(8)]], const device uint* raw_batches [[buffer(9)]],
    constant BufferSizes& sizes [[buffer(29)]], uint group [[threadgroup_position_in_grid]], uint lane [[thread_index_in_threadgroup]]) {
    Words draws{raw_draws, word_size(sizes, 1)};
    Words text{raw_text, word_size(sizes, 2)};
    Words paths{raw_paths, word_size(sizes, 3)};
    Words backdrops{raw_backdrops, word_size(sizes, 4)};
    Words layers{raw_layers, word_size(sizes, 6)};
    Words sdf{raw_sdf, word_size(sizes, 8)};
    Words batches{raw_batches, word_size(sizes, 9)};

    if (group >= c.active_tile_count) return;
    uint tile = tile_at(work, c, group);
    if (tile >= c.tile_count) return;
    uint2 position(tile % c.tiles_width, tile / c.tiles_width);
    uint wrappers = wrapper_count(c, layers, draws, paths, backdrops, ranges, sdf, position);
    uint2 count(0), total;
    if (wrappers != invalid_index) {
        uint base = tile_draw_base(c, tile), page = work[base], remaining = work[base + 1];
        while (page != invalid_index && remaining) {
            if (lane < min(remaining, 256u))
                count += particle_count(c, draws, text, paths, backdrops, ranges, batches, page_index(work, c, page, lane), position);
            remaining -= min(remaining, 256u);
            page = page_next(work, c, page);
        }
    }
    threadgroup uint2 scratch[256];
    exclusive_prefix(count, lane, scratch, total);
    if (!lane) {
        if (total.x) total.x += wrappers * 2 + 1;
        store_counts(work, c, tile, total);
    }
}
kernel void coarse_count_bins(constant CoarseConfig& c [[buffer(0)]],
    const device uint* raw_draws [[buffer(1)]], const device uint* raw_text [[buffer(2)]],
    const device uint* raw_paths [[buffer(3)]], const device uint* raw_backdrops [[buffer(4)]],
    const device packed_uint2* ranges [[buffer(5)]], const device uint* raw_layers [[buffer(6)]],
    device uint* work [[buffer(7)]], const device uint* raw_sdf [[buffer(8)]], const device uint* raw_batches [[buffer(9)]],
    constant BufferSizes& sizes [[buffer(29)]], uint group [[threadgroup_position_in_grid]], uint lane [[thread_index_in_threadgroup]]) {
    Words draws{raw_draws, word_size(sizes, 1)};
    Words text{raw_text, word_size(sizes, 2)};
    Words paths{raw_paths, word_size(sizes, 3)};
    Words backdrops{raw_backdrops, word_size(sizes, 4)};
    Words layers{raw_layers, word_size(sizes, 6)};
    Words sdf{raw_sdf, word_size(sizes, 8)};
    Words batches{raw_batches, word_size(sizes, 9)};

    uint bins = (c.tiles_width + 15) / 16;
    uint2 position = uint2(group % bins, group / bins) * 16 + uint2(lane % 16, lane / 16);
    if (position.x >= c.tiles_width || position.y >= c.tiles_height) return;
    uint tile = position.y * c.tiles_width + position.x;
    if (tile >= c.tile_count) return;
    uint wrappers = wrapper_count(c, layers, draws, paths, backdrops, ranges, sdf, position);
    uint2 count(0);
    if (wrappers != invalid_index) {
        uint base = tile_draw_base(c, tile), page = work[base], remaining = work[base + 1];
        while (page != invalid_index && remaining) {
            uint n = min(remaining, 256u);
            for (uint i = 0; i < n; ++i)
                count += particle_count(c, draws, text, paths, backdrops, ranges, batches, page_index(work, c, page, i), position);
            remaining -= n;
            page = page_next(work, c, page);
        }
    }
    if (count.x) count.x += wrappers * 2 + 1;
    store_counts(work, c, tile, count);
}
