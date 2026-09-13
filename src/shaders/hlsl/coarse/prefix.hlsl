#include "../coarse_records.hlsli"
#include "../constants.hlsli"
#include "config.hlsli"
RWByteAddressBuffer coarse_work : register(u7, space0);
RWByteAddressBuffer chunk_records : register(u8, space0);

#include "prefix_scan.hlsli"

uint item_count() { return config.incremental != 0u ? config.active_tile_count : config.tile_count; }
uint tile_at(uint item) {
    return config.incremental != 0u ? coarse_work.Load((config.active_tile_list_base + item) * 4u) : item;
}

[numthreads(COARSE_WORKGROUP_SIZE, 1, 1)]
void coarse_prefix_chunks(uint3 group : SV_GroupID, uint3 local : SV_GroupThreadID) {
    // Logical chunks bound raw metadata independently of retained capacity.
    if (group.x >= config.chunk_count) return;
    uint item = group.x * COARSE_WORKGROUP_SIZE + local.x;
    uint tile_base = 0u;
    uint2 count = uint2(0u, 0u);
    if (item < item_count()) {
        tile_base = tile_at(item) * COARSE_TILE_RECORD_STRIDE;
        count = uint2(coarse_work.Load(tile_base), coarse_work.Load(tile_base + COARSE_TILE_GLYPH_COUNT));
    }
    uint2 start = exclusive_prefix(count, local.x);
    uint chunk_base = group.x * COARSE_CHUNK_RECORD_STRIDE;
    if (local.x == 0u) {
        chunk_records.Store(chunk_base, block_total.x);
        chunk_records.Store(chunk_base + COARSE_CHUNK_GLYPH_TOTAL, block_total.y);
    }
    if (item < item_count()) {
        coarse_work.Store2(tile_base + COARSE_TILE_PTCL_START, uint2(start.x, start.x + count.x));
        coarse_work.Store2(tile_base + COARSE_TILE_GLYPH_START, uint2(start.y, start.y + count.y));
    }
}

[numthreads(COARSE_WORKGROUP_SIZE, 1, 1)]
void coarse_chunk_offsets(uint3 local : SV_GroupThreadID) {
    uint2 carry = uint2(0u, 0u);
    for (uint block = 0u; block < config.chunk_count; block += COARSE_WORKGROUP_SIZE) {
        uint chunk = block + local.x;
        uint base = chunk * COARSE_CHUNK_RECORD_STRIDE;
        uint2 count = uint2(0u, 0u);
        if (chunk < config.chunk_count) count = uint2(chunk_records.Load(base), chunk_records.Load(base + COARSE_CHUNK_GLYPH_TOTAL));
        uint2 offset = exclusive_prefix(count, local.x);
        if (chunk < config.chunk_count) {
            chunk_records.Store(base + COARSE_CHUNK_PTCL_OFFSET, carry.x + offset.x);
            chunk_records.Store(base + COARSE_CHUNK_GLYPH_OFFSET, carry.y + offset.y);
        }
        carry += block_total;
    }
}

[numthreads(COARSE_WORKGROUP_SIZE, 1, 1)]
void coarse_apply_chunk_offsets(uint3 group : SV_GroupID, uint3 local : SV_GroupThreadID) {
    uint item = group.x * COARSE_WORKGROUP_SIZE + local.x;
    if (item >= item_count()) return;
    uint tile_base = tile_at(item) * COARSE_TILE_RECORD_STRIDE;
    uint chunk_base = group.x * COARSE_CHUNK_RECORD_STRIDE;
    uint ptcl = chunk_records.Load(chunk_base + COARSE_CHUNK_PTCL_OFFSET);
    uint glyph = chunk_records.Load(chunk_base + COARSE_CHUNK_GLYPH_OFFSET);
    coarse_work.Store2(tile_base + COARSE_TILE_PTCL_START, coarse_work.Load2(tile_base + COARSE_TILE_PTCL_START) + ptcl);
    coarse_work.Store2(tile_base + COARSE_TILE_GLYPH_START, coarse_work.Load2(tile_base + COARSE_TILE_GLYPH_START) + glyph);
}
