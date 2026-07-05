/// GPU-visible per-path geometry and scan allocation metadata.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PathRecord {
    pub path_id: u32,
    pub line_count: u32,
    pub line_start: u32,
    /// Reserved path-level scan flags for GPU-visible pipeline metadata.
    pub flags: u32,
    pub data_offset: u32,
    pub data_len: u32,
    pub tile_x0: u32,
    pub tile_y0: u32,
    pub tile_x1: u32,
    pub tile_y1: u32,
    pub segment_start: u32,
    pub segment_capacity: u32,
    pub segment_count: u32,
}
