#include "../coarse_records.hlsli"
#include "../constants.hlsli"
#include "config.hlsli"
#include "prefix_scan.hlsli"
#include "emit_layout.hlsli"

ConstantBuffer<CoarseConfig> config : register(b0, space0);
RWByteAddressBuffer coarse_work : register(u7, space0);
RWByteAddressBuffer chunk_records : register(u8, space0);

[numthreads(COARSE_WORKGROUP_SIZE, 1, 1)]
void coarse_emit_chunk_counts(uint3 id : SV_DispatchThreadID) {
    if (id.x >= config.tile_count) return;
    uint count = coarse_work.Load(tile_draw_base(config, id.x) + COARSE_TILE_DRAW_COUNT);
    coarse_work.Store(tile_emit_base(config, id.x), (count + COARSE_WORKGROUP_SIZE - 1u) / COARSE_WORKGROUP_SIZE);
}

[numthreads(COARSE_WORKGROUP_SIZE, 1, 1)]
void coarse_emit_prefix_chunks(uint3 group : SV_GroupID, uint3 local : SV_GroupThreadID) {
    if (group.x >= config.chunk_count) return;
    uint tile = group.x * COARSE_WORKGROUP_SIZE + local.x;
    uint count = 0u;
    if (tile < config.tile_count) count = coarse_work.Load(tile_emit_base(config, tile));
    uint2 total;
    uint2 offset = exclusive_prefix(uint2(count, 0u), local.x, total);
    if (local.x == 0u) chunk_records.Store(group.x * COARSE_CHUNK_RECORD_STRIDE, total.x);
    if (tile < config.tile_count) coarse_work.Store(tile_emit_base(config, tile) + 4u, offset.x);
}

[numthreads(1, 1, 1)]
void coarse_emit_chunk_offsets() {
    uint carry = 0u;
    for (uint chunk = 0u; chunk < config.chunk_count; chunk++) {
        uint base = chunk * COARSE_CHUNK_RECORD_STRIDE;
        chunk_records.Store(base + COARSE_CHUNK_PTCL_OFFSET, carry);
        carry += chunk_records.Load(base);
    }
}

[numthreads(COARSE_WORKGROUP_SIZE, 1, 1)]
void coarse_emit_apply_chunk_offsets(uint3 id : SV_DispatchThreadID) {
    if (id.x >= config.tile_count) return;
    uint base = tile_emit_base(config, id.x) + 4u;
    uint chunk_base = (id.x / COARSE_WORKGROUP_SIZE) * COARSE_CHUNK_RECORD_STRIDE;
    coarse_work.Store(base, coarse_work.Load(base) + chunk_records.Load(chunk_base + COARSE_CHUNK_PTCL_OFFSET));
}

[numthreads(COARSE_WORKGROUP_SIZE, 1, 1)]
void coarse_emit_fill_refs(uint3 id : SV_DispatchThreadID) {
    if (id.x >= config.tile_count) return;
    uint2 record = coarse_work.Load2(tile_emit_base(config, id.x));
    for (uint local = 0u; local < record.x; local++) {
        uint reference = record.y + local;
        if (reference < config.emit_chunk_capacity) {
            uint base = emit_base(config, reference);
            coarse_work.Store2(base, uint2(id.x, local));
            coarse_work.Store(base + COARSE_EMIT_CLASS_FLAGS, 0u);
        }
    }
}
