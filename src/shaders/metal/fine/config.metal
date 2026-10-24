struct FineConfig {
    uint width,height,clear_color,tile_count,tiles_width,tiles_height,load_target;
    uint clip_spill_depth,group_spill_depth,ptcl_capacity,paint_sdf_shadow_base,paint_brush_base;
    uint text_image_base,text_image_data_base,group_spill_base,fine_tile_kind_base;
    uint active_tile_count,dispatch_width,active_tile_list_base,incremental;
};
static_assert(sizeof(FineConfig)==80,"FineConfig ABI");
