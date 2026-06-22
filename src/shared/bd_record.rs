use bytemuck::{Pod, Zeroable};

/// each path has one backdrop record
///
/// path {
///  list of tiles the path covers {
///     each tile has list of line segments
/// }
/// }
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct BackdropRecord {
    pub path_id: u32,
    /// 在 backdrop_pool 里的起始下标（i32 元素）
    pub data_offset: u32,
    /// 全图画布上的 tile 原点（scan_assign 里 bbox）
    pub tile_x0: u32,
    pub tile_y0: u32,
    pub tile_x1: u32, // exclusive
    pub tile_y1: u32,
    /// 全局 segment buffer 内该 path 的起始位置，每个segment由lines[]里的line经过对tile进行相交clip得到
    pub segment_start: u32,
    pub segment_capacity: u32,
    /// scan 之后填：实际写出的 segment 数
    pub segment_count: u32,
}
