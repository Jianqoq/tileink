#include "tags.hlsli"
#include "../coarse_records.hlsli"
#include "../constants.hlsli"
#include "config.hlsli"
#include "classify.hlsli"
#include "draw_list.hlsli"
#include "tile_classification.hlsli"

ConstantBuffer<CoarseConfig> config : register(b0, space0);
ByteAddressBuffer draw_records : register(t1, space0);
ByteAddressBuffer sdf_blob : register(t3, space0);
ByteAddressBuffer path_records : register(t4, space0);
ByteAddressBuffer backdrops : register(t5, space0);
ByteAddressBuffer segment_ranges : register(t6, space0);
ByteAddressBuffer layer_stack : register(t7, space0);
RWByteAddressBuffer coarse_work : register(u8, space0);

[numthreads(COARSE_WORKGROUP_SIZE, 1, 1)]
void coarse_emit_chunk_tile_kinds(uint3 id : SV_DispatchThreadID) {
    if (id.x >= config.tile_count) return;
    uint kind = TILE_KIND_EMPTY;
    if (coarse_work.Load(id.x * COARSE_TILE_RECORD_STRIDE) != 0u) {
        uint2 tile = uint2(id.x % config.tiles_width, id.x / config.tiles_width);
        uint wrappers = stack_wrapper_count(config, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, tile);
        uint flags = wrappers != 0u ? CHUNK_CLASS_OTHER : 0u;
        // Invalid or retained stacks require the interpreter. Empty tiles above
        // remain empty regardless of stale reference flags or stack contents.
        if (wrappers != INVALID_INDEX) {
            uint2 range = coarse_work.Load2(tile_emit_base(config, id.x));
            for (uint chunk = 0u; chunk < range.x; chunk++)
                flags |= coarse_work.Load(emit_base(config, range.y + chunk) + COARSE_EMIT_CLASS_FLAGS);
        }
        kind = classify_tile_flags(flags);
    }
    coarse_work.Store(emit_base(config, config.emit_chunk_capacity) + id.x * 4u, kind);
}
