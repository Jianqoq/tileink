#include "../coarse_records.hlsli"
#include "../constants.hlsli"
#include "config.hlsli"
#include "classify.hlsli"
#include "draw_list.hlsli"

ConstantBuffer<CoarseConfig> config : register(b0, space0);
ByteAddressBuffer draw_records : register(t1, space0);
ByteAddressBuffer path_records : register(t3, space0);
ByteAddressBuffer backdrops : register(t4, space0);
ByteAddressBuffer segment_ranges : register(t5, space0);
ByteAddressBuffer layer_stack : register(t6, space0);
RWByteAddressBuffer coarse_work : register(u7, space0);
ByteAddressBuffer sdf_blob : register(t9, space0);

[numthreads(COARSE_WORKGROUP_SIZE, 1, 1)]
void coarse_tile_counts_from_emit_chunks(uint3 id : SV_DispatchThreadID) {
    if (id.x >= config.tile_count) return;
    uint2 range = coarse_work.Load2(tile_emit_base(config, id.x));
    uint2 count = uint2(0u, 0u);
    for (uint chunk = 0u; chunk < range.x; chunk++) {
        uint base = emit_base(config, range.y + chunk);
        count += uint2(coarse_work.Load(base + COARSE_EMIT_PTCL_COUNT), coarse_work.Load(base + COARSE_EMIT_GLYPH_COUNT));
    }
    // Wrappers surround nonempty tile streams; invalid stacks suppress both streams.
    if (count.x > 0u) {
        uint2 position = uint2(id.x % config.tiles_width, id.x / config.tiles_width);
        uint wrappers = stack_wrapper_count(config, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, position);
        if (wrappers == INVALID_INDEX) count = uint2(0u, 0u);
        else count.x += wrappers * 2u + 1u;
    }
    store_tile_counts(coarse_work, config, id.x, count);
}
