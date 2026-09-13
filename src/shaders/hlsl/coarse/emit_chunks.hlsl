#include "../dispatch.hlsli"
#include "../constants.hlsli"
#include "config.hlsli"
#include "active_tiles.hlsli"
#include "draw_list.hlsli"
#include "prefix_scan.hlsli"
#include "emit_draw.hlsli"
#include "emit_stack.hlsli"

ConstantBuffer<CoarseConfig> config : register(b0, space0);
ByteAddressBuffer draw_records : register(t1, space0);
ByteAddressBuffer text_blob : register(t2, space0);
ByteAddressBuffer sdf_blob : register(t3, space0);
ByteAddressBuffer path_records : register(t4, space0);
ByteAddressBuffer backdrops : register(t5, space0);
ByteAddressBuffer segment_ranges : register(t6, space0);
ByteAddressBuffer layer_stack : register(t7, space0);
RWByteAddressBuffer coarse_work : register(u8, space0);
ByteAddressBuffer draw_batch_ids : register(t9, space0);

ConstantBuffer<DispatchGrid> dispatch_grid : register(b31, space0);
groupshared uint emission_flags[COARSE_WORKGROUP_SIZE];

[numthreads(COARSE_WORKGROUP_SIZE,1,1)]
void coarse_emit_chunks(uint3 group : SV_GroupID, uint3 local : SV_GroupThreadID) {
    if (config.tile_count == 0u) return;
    uint reference = linear_group(group, uint2(dispatch_grid.x, dispatch_grid.y));
    uint2 last_range = coarse_work.Load2(tile_emit_base(config, config.tile_count - 1u));
    // Neither spare physical capacity nor a rounded-up grid is a live reference.
    if (reference >= config.emit_chunk_capacity || reference >= last_range.x + last_range.y) return;
    uint base = emit_base(config, reference);
    uint4 chunk = coarse_work.Load4(base);
    uint2 chunk_glyphs = coarse_work.Load2(base + COARSE_EMIT_GLYPH_COUNT);
    uint tile = chunk.x;
    uint2 position = uint2(tile % config.tiles_width, tile / config.tiles_width);
    uint2 particles = coarse_work.Load2(tile * COARSE_TILE_RECORD_STRIDE + COARSE_TILE_PTCL_START);
    uint2 glyphs = coarse_work.Load2(tile * COARSE_TILE_RECORD_STRIDE + COARSE_TILE_GLYPH_START);
    uint wrappers = stack_wrapper_count(config, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, position);
    bool emit_wrappers = particles.x < particles.y && wrappers != INVALID_INDEX;
    if (local.x == 0u && emit_wrappers && chunk.y == 0u)
        emit_stack_begins(config, coarse_work, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, particles.x, position);
    particles.x += (emit_wrappers ? wrappers : 0u) + chunk.w;
    glyphs.x += chunk_glyphs.y;
    uint page = draw_page_at(coarse_work, config, tile, chunk.y);
    uint ordinal = chunk.y * COARSE_WORKGROUP_SIZE + local.x;
    uint count = coarse_work.Load(tile_draw_base(config, tile) + COARSE_TILE_DRAW_COUNT);
    uint draw_index = INVALID_INDEX;
    if (page != INVALID_INDEX && ordinal < count) draw_index = draw_page_index(coarse_work, config, page, local.x);
    Particle particle = empty_particle();
    if (emit_wrappers) particle = draw_particle(config, draw_records, text_blob, sdf_blob, path_records, backdrops, segment_ranges, draw_batch_ids, draw_index, position);
    uint2 total;
    uint2 offset = exclusive_prefix(uint2(particle.valid ? 1u : 0u, particle.glyph_count), local.x, total);
    uint flags = 0u;
    if (particle.valid) {
        CoarseDraw draw = load_draw(draw_records, draw_index);
        flags = particle_class_flags(sdf_blob, config, draw, particle.tag);
        if (particle.tag == PTCL_GLYPH) {
            particle.segments = uint2(glyphs.x + offset.y, glyphs.x + offset.y + particle.glyph_count);
            if (particle.segments.y <= glyphs.y) store_draw_glyphs(coarse_work, text_blob, config, draw, position, particle.segments.x);
        }
        store_particle_value(coarse_work, config, particles.x + offset.x, particle);
    }
    if (local.x == 0u && emit_wrappers && chunk.y + 1u == coarse_work.Load(tile_emit_base(config, tile))) {
        uint end_cursor = particles.x + chunk.z;
        emit_stack_ends(config, coarse_work, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, end_cursor, position);
        store_particle(coarse_work, config, end_cursor + wrappers, PTCL_END, 0u, 0u, uint2(0u,0u), 0u);
    }
    // OR reduction preserves classification membership without three separate scans.
    emission_flags[local.x] = flags;
    GroupMemoryBarrierWithGroupSync();
    for (uint stride = COARSE_WORKGROUP_SIZE / 2u; stride != 0u; stride /= 2u) {
        if (local.x < stride) emission_flags[local.x] |= emission_flags[local.x + stride];
        GroupMemoryBarrierWithGroupSync();
    }
    if (local.x == 0u) coarse_work.Store(base + COARSE_EMIT_CLASS_FLAGS, emission_flags[0]);
}
