#include "../constants.hlsli"
#include "config.hlsli"
#include "classify.hlsli"
#include "draw_list.hlsli"
#include "active_tiles.hlsli"
#include "prefix_scan.hlsli"

ConstantBuffer<CoarseConfig> config : register(b0, space0);
ByteAddressBuffer draw_records : register(t1, space0);
ByteAddressBuffer text_blob : register(t2, space0);
ByteAddressBuffer path_records : register(t3, space0);
ByteAddressBuffer backdrops : register(t4, space0);
ByteAddressBuffer segment_ranges : register(t5, space0);
ByteAddressBuffer layer_stack : register(t6, space0);
RWByteAddressBuffer coarse_work : register(u7, space0);
ByteAddressBuffer sdf_blob : register(t8, space0);
ByteAddressBuffer draw_batch_ids : register(t9, space0);

[numthreads(COARSE_WORKGROUP_SIZE,1,1)]
void coarse_count(uint3 group : SV_GroupID, uint3 local : SV_GroupThreadID) {
    if (group.x >= config.active_tile_count) return;
    uint tile = coarse_tile_at(coarse_work, config, group.x);
    if (tile >= config.tile_count) return;
    uint2 position = uint2(tile % config.tiles_width, tile / config.tiles_width);
    uint wrappers = stack_wrapper_count(config, layer_stack, draw_records, path_records, backdrops, segment_ranges, sdf_blob, position);
    uint2 count = uint2(0u,0u);
    if (wrappers != INVALID_INDEX) {
        uint2 list = coarse_work.Load2(tile_draw_base(config,tile));
        uint page = list.x, remaining = list.y;
        while (page != INVALID_INDEX && remaining != 0u) {
            if (local.x < min(remaining, COARSE_WORKGROUP_SIZE)) {
                uint draw_index = draw_page_index(coarse_work,config,page,local.x);
                count += draw_particle_count(config,draw_records,text_blob,path_records,backdrops,segment_ranges,draw_batch_ids,draw_index,position);
            }
            remaining -= min(remaining,COARSE_WORKGROUP_SIZE);
            page = draw_page_next(coarse_work,config,page);
        }
    }
    uint2 total;
    exclusive_prefix(count,local.x,total);
    if (local.x == 0u) {
        if (total.x > 0u) total.x += wrappers * 2u + 1u;
        store_tile_counts(coarse_work,config,tile,total);
    }
}

[numthreads(COARSE_WORKGROUP_SIZE,1,1)]
void coarse_count_bins(uint3 group : SV_GroupID, uint3 local : SV_GroupThreadID) {
    uint bins_per_row = (config.tiles_width + COARSE_BIN_SIDE - 1u) / COARSE_BIN_SIDE;
    uint2 position = uint2(group.x % bins_per_row, group.x / bins_per_row) * COARSE_BIN_SIDE
        + uint2(local.x % COARSE_BIN_SIDE, local.x / COARSE_BIN_SIDE);
    if (position.x >= config.tiles_width || position.y >= config.tiles_height) return;
    uint tile = position.y * config.tiles_width + position.x;
    if (tile >= config.tile_count) return;
    uint wrappers = stack_wrapper_count(config,layer_stack,draw_records,path_records,backdrops,segment_ranges,sdf_blob,position);
    uint2 count = uint2(0u,0u);
    if (wrappers != INVALID_INDEX) {
        uint2 list = coarse_work.Load2(tile_draw_base(config,tile));
        uint page=list.x, remaining=list.y;
        while (page != INVALID_INDEX && remaining != 0u) {
            uint page_count=min(remaining,COARSE_WORKGROUP_SIZE);
            for (uint slot=0u;slot<page_count;slot++) {
                uint draw_index=draw_page_index(coarse_work,config,page,slot);
                count += draw_particle_count(config,draw_records,text_blob,path_records,backdrops,segment_ranges,draw_batch_ids,draw_index,position);
            }
            remaining -= page_count;
            page = draw_page_next(coarse_work,config,page);
        }
    }
    if (count.x>0u) count.x += wrappers*2u+1u;
    store_tile_counts(coarse_work,config,tile,count);
}
