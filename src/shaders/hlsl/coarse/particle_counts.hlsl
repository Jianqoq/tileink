#include "../coarse_records.hlsli"
#include "../constants.hlsli"
#include "../dispatch.hlsli"
#include "config.hlsli"
#include "classify.hlsli"
#include "draw_list.hlsli"
#include "prefix_scan.hlsli"

ConstantBuffer<CoarseConfig> config : register(b0, space0);
ConstantBuffer<DispatchGrid> dispatch_grid : register(b31, space0);
ByteAddressBuffer draw_records : register(t1, space0);
ByteAddressBuffer text_blob : register(t2, space0);
ByteAddressBuffer path_records : register(t3, space0);
ByteAddressBuffer backdrops : register(t4, space0);
ByteAddressBuffer segment_ranges : register(t5, space0);
ByteAddressBuffer layer_stack : register(t6, space0);
RWByteAddressBuffer coarse_work : register(u7, space0);
ByteAddressBuffer sdf_blob : register(t9, space0);
ByteAddressBuffer draw_batch_ids : register(t10, space0);

[numthreads(COARSE_WORKGROUP_SIZE, 1, 1)]
void coarse_emit_chunk_particle_counts(uint3 group : SV_GroupID, uint3 local : SV_GroupThreadID) {
    if (config.tile_count == 0u) return;
    uint reference = linear_group(group, uint2(dispatch_grid.x, dispatch_grid.y));
    // Prefix allocation makes tile ranges contiguous. Capacity is not the live
    // count: padded groups must not process stale records in a reused buffer.
    uint2 last_range = coarse_work.Load2(tile_emit_base(config, config.tile_count - 1u));
    if (reference >= last_range.x + last_range.y) return;
    uint base = emit_base(config, reference);
    uint2 chunk = coarse_work.Load2(base);
    uint2 position = uint2(chunk.x % config.tiles_width, chunk.x / config.tiles_width);
    uint wrappers = stack_wrapper_count(config, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, position);
    uint page = draw_page_at(coarse_work, config, chunk.x, chunk.y);
    uint ordinal = chunk.y * COARSE_WORKGROUP_SIZE + local.x;
    uint draw_count = coarse_work.Load(tile_draw_base(config, chunk.x) + COARSE_TILE_DRAW_COUNT);
    uint2 count = uint2(0u, 0u);
    if (wrappers != INVALID_INDEX && page != INVALID_INDEX && ordinal < draw_count) {
        uint draw_index = draw_page_index(coarse_work, config, page, local.x);
        count = draw_particle_count(config, draw_records, text_blob, path_records, backdrops, segment_ranges, draw_batch_ids, draw_index, position);
    }
    uint2 total;
    exclusive_prefix(count, local.x, total);
    if (local.x == 0u) {
        coarse_work.Store(base + COARSE_EMIT_PTCL_COUNT, total.x);
        coarse_work.Store(base + COARSE_EMIT_GLYPH_COUNT, total.y);
    }
}
