#ifndef TILEINK_HLSL_FINE_CONFIG_HLSLI_INCLUDED
#define TILEINK_HLSL_FINE_CONFIG_HLSLI_INCLUDED
struct FineConfig {
    uint width;
    uint height;
    uint clear_color;
    uint tile_count;
    uint tiles_width;
    uint tiles_height;
    uint load_target;
    uint clip_spill_depth;
    uint group_spill_depth;
    uint ptcl_capacity;
    uint paint_sdf_shadow_base;
    uint paint_brush_base;
    uint text_image_base;
    uint text_image_data_base;
    uint group_spill_base;
    uint fine_tile_kind_base;
    uint active_tile_count;
    uint dispatch_width;
    uint active_tile_list_base;
    uint incremental;
};
#endif // TILEINK_HLSL_FINE_CONFIG_HLSLI_INCLUDED
