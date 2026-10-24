#ifndef TILEINK_HLSL_COARSE_ACTIVE_TILES_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_ACTIVE_TILES_HLSLI_INCLUDED

#include "config.hlsli"

uint coarse_item_count(ConstantBuffer<CoarseConfig> settings) { return settings.incremental != 0u ? settings.active_tile_count : settings.tile_count; }
uint coarse_tile_at(RWByteAddressBuffer work, ConstantBuffer<CoarseConfig> settings, uint item) {
    return settings.incremental != 0u ? work.Load((settings.active_tile_list_base + item) * 4u) : item;
}

#endif // TILEINK_HLSL_COARSE_ACTIVE_TILES_HLSLI_INCLUDED
