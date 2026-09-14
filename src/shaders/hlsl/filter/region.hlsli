#ifndef TILEINK_FILTER_REGION_HLSLI
#define TILEINK_FILTER_REGION_HLSLI
#include "config.hlsli"
#include "../constants.hlsli"

// Keep the constant-buffer type explicit: Shader Tools does not unwrap it for helper calls.
// Bounds precede raw active-tile reads; each compact tile owns its pixel lanes.
bool filter_position(ConstantBuffer<FilterConfig> config, ByteAddressBuffer active_tiles, uint3 gid, out uint2 xy) {
    uint region_ix = gid.x + gid.y * config.dispatch_width * FILTER_WORKGROUP_SIZE;
    xy = 0;
    if (region_ix >= config.pixel_count) return false;
    if (config.compact_tiles == 0u) {
        xy = uint2(config.region_x0 + region_ix % config.region_width,
                   config.region_y0 + region_ix / config.region_width);
        return true;
    }
    uint tile_list_ix = region_ix / FINE_WORKGROUP_SIZE;
    if (tile_list_ix >= config.active_tile_count) return false;
    uint lane = region_ix % FINE_WORKGROUP_SIZE;
    uint tile = active_tiles.Load(tile_list_ix * 4u);
    xy = uint2(tile % config.tiles_width, tile / config.tiles_width) * TILE_SIZE
        + uint2(lane % TILE_SIZE, lane / TILE_SIZE);
    return all(xy < uint2(config.width, config.height))
        && all(xy >= uint2(config.region_x0, config.region_y0))
        && all(xy < uint2(config.region_x0 + config.region_width, config.region_y0 + config.region_height));
}

bool filter_contains(ConstantBuffer<FilterConfig> config, int2 xy) {
    return all(xy >= int2(config.region_x0, config.region_y0))
        && all(xy < int2(config.region_x0 + config.region_width, config.region_y0 + config.region_height));
}
#endif
