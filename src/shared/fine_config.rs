// Common host uniform for wgpu and native fine rendering.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct FineConfig {
    pub width: u32,
    pub height: u32,
    pub clear_color: u32,
    pub tile_count: u32,
    pub tiles_width: u32,
    pub tiles_height: u32,
    pub load_target: u32,
    pub clip_spill_depth: u32,
    pub group_spill_depth: u32,
    pub ptcl_capacity: u32,
    pub paint_sdf_shadow_base: u32,
    pub paint_brush_base: u32,
    pub text_image_base: u32,
    pub text_image_data_base: u32,
    pub group_spill_base: u32,
    pub fine_tile_kind_base: u32,
    pub active_tile_count: u32,
    pub dispatch_width: u32,
    pub active_tile_list_base: u32,
    pub incremental: u32,
}
