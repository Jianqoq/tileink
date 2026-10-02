#include "tags.hlsli"
#include "../constants.hlsli"
#include "config.hlsli"
#include "active_tiles.hlsli"
#include "draw_list.hlsli"
#include "prefix_scan.hlsli"
#include "emit_draw.hlsli"
#include "emit_stack.hlsli"
#include "tile_classification.hlsli"

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

[numthreads(COARSE_WORKGROUP_SIZE,1,1)]
void coarse_emit(uint3 group : SV_GroupID, uint3 local : SV_GroupThreadID) {
    if (group.x >= config.active_tile_count) return;
    uint tile = coarse_tile_at(coarse_work, config, group.x);
    if (tile >= config.tile_count) return;
    uint2 position = uint2(tile % config.tiles_width, tile / config.tiles_width);
    uint2 particles = coarse_work.Load2(tile * COARSE_TILE_RECORD_STRIDE + COARSE_TILE_PTCL_START);
    uint2 glyphs = coarse_work.Load2(tile * COARSE_TILE_RECORD_STRIDE + COARSE_TILE_GLYPH_START);
    // A preceding scalar clip batch may have classified the reused tile as
    // EMPTY/COLOR. Parallel emission always produces an interpreter stream.
    if (local.x == 0u) {
        uint kind = particles.x < particles.y ? TILE_KIND_INTERPRETER : TILE_KIND_EMPTY;
        coarse_work.Store(emit_base(config, config.emit_chunk_capacity) + tile * 4u, kind);
    }
    if (particles.x >= particles.y) return;
    uint wrappers = stack_wrapper_count(config, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, position);
    if (wrappers == INVALID_INDEX) {
        // A preallocated tile range may still contain an earlier batch's particles.
        if (local.x == 0u) store_particle(coarse_work, config, particles.x, PTCL_END, 0u, 0u, uint2(0u,0u), 0u);
        return;
    }
    if (local.x == 0u) emit_stack_begins(config, coarse_work, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, particles.x, position);
    particles.x += wrappers;
    uint2 list = coarse_work.Load2(tile_draw_base(config, tile));
    uint page = list.x, remaining = list.y;
    while (page != INVALID_INDEX && remaining != 0u) {
        uint page_count = min(remaining, COARSE_WORKGROUP_SIZE);
        uint draw_index = INVALID_INDEX;
        if (local.x < page_count) draw_index = draw_page_index(coarse_work, config, page, local.x);
        Particle particle = draw_particle(config, draw_records, text_blob, sdf_blob, path_records, backdrops, segment_ranges, draw_batch_ids, draw_index, position);
        uint2 total;
        uint2 offset = exclusive_prefix(uint2(particle.valid ? 1u : 0u, particle.glyph_count), local.x, total);
        if (particle.valid) {
            if (particle.tag == PTCL_GLYPH) {
                particle.segments = uint2(glyphs.x + offset.y, glyphs.x + offset.y + particle.glyph_count);
                if (particle.segments.y <= glyphs.y) store_draw_glyphs(coarse_work, text_blob, config, load_draw(draw_records, draw_index), position, particle.segments.x);
            }
            store_particle_value(coarse_work, config, particles.x + offset.x, particle);
        }
        particles.x += total.x;
        glyphs.x += total.y;
        remaining -= page_count;
        page = draw_page_next(coarse_work, config, page);
    }
    if (local.x == 0u) {
        emit_stack_ends(config, coarse_work, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, particles.x, position);
        store_particle(coarse_work, config, particles.x + wrappers, PTCL_END, 0u, 0u, uint2(0u,0u), 0u);
    }
}

[numthreads(COARSE_WORKGROUP_SIZE,1,1)]
void coarse_emit_bins(uint3 group : SV_GroupID, uint3 local : SV_GroupThreadID) {
    uint tile;
    uint2 position;
    if (config.incremental != 0u) {
        // Preallocated clips assign one lane to each selected tile, avoiding
        // a whole workgroup and repeated prefix scans for a short draw list.
        uint active = group.x * COARSE_WORKGROUP_SIZE + local.x;
        if (active >= config.active_tile_count) return;
        tile = coarse_tile_at(coarse_work, config, active);
        if (tile >= config.tile_count) return;
        position = uint2(tile % config.tiles_width, tile / config.tiles_width);
    } else {
        uint bins_per_row = (config.tiles_width + COARSE_BIN_SIDE - 1u) / COARSE_BIN_SIDE;
        position = uint2(group.x % bins_per_row, group.x / bins_per_row) * COARSE_BIN_SIDE + uint2(local.x % COARSE_BIN_SIDE, local.x / COARSE_BIN_SIDE);
        if (position.x >= config.tiles_width || position.y >= config.tiles_height) return;
        tile = position.y * config.tiles_width + position.x;
    }
    if (tile >= config.tile_count) return;
    uint kind_base = emit_base(config, config.emit_chunk_capacity) + tile * 4u;
    uint2 particles = coarse_work.Load2(tile * COARSE_TILE_RECORD_STRIDE + COARSE_TILE_PTCL_START);
    uint2 glyphs = coarse_work.Load2(tile * COARSE_TILE_RECORD_STRIDE + COARSE_TILE_GLYPH_START);
    if (particles.x >= particles.y) { coarse_work.Store(kind_base, TILE_KIND_EMPTY); return; }
    uint wrappers = stack_wrapper_count(config, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, position);
    if (wrappers == INVALID_INDEX) {
        // Rejected clips must not expose a previous batch's reused stream.
        store_particle(coarse_work, config, particles.x, PTCL_END, 0u, 0u, uint2(0u,0u), 0u);
        coarse_work.Store(kind_base, TILE_KIND_EMPTY);
        return;
    }
    uint flags = wrappers != 0u ? CHUNK_CLASS_OTHER : 0u;
    emit_stack_begins(config, coarse_work, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, particles.x, position);
    particles.x += wrappers;
    uint2 list = coarse_work.Load2(tile_draw_base(config, tile));
    uint page = list.x, remaining = list.y, slot = 0u;
    while (page != INVALID_INDEX && remaining != 0u) {
        uint draw_index = draw_page_index(coarse_work, config, page, slot);
        Particle particle = draw_particle(config, draw_records, text_blob, sdf_blob, path_records, backdrops, segment_ranges, draw_batch_ids, draw_index, position);
        if (particle.valid) {
            DrawData draw = load_draw(draw_records, draw_index);
            flags |= particle_class_flags(sdf_blob, config, draw, particle.tag);
            if (particle.tag == PTCL_GLYPH) {
                particle.segments = uint2(glyphs.x, glyphs.x + particle.glyph_count);
                if (particle.segments.y <= glyphs.y) store_draw_glyphs(coarse_work, text_blob, config, draw, position, particle.segments.x);
                glyphs.x = particle.segments.y;
            }
            if (particles.x < particles.y) store_particle_value(coarse_work, config, particles.x++, particle);
        }
        slot++;
        remaining--;
        if (slot == COARSE_WORKGROUP_SIZE) { page = draw_page_next(coarse_work, config, page); slot = 0u; }
    }
    emit_stack_ends(config, coarse_work, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, particles.x, position);
    store_particle(coarse_work, config, particles.x + wrappers, PTCL_END, 0u, 0u, uint2(0u,0u), 0u);
    coarse_work.Store(kind_base, classify_tile_flags(flags));
}
