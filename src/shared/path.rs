/// GPU-visible per-path geometry and scan allocation metadata.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PathRecord {
    pub path_id: u32,
    pub line_count: u32,
    pub line_start: u32,
    /// Path-level scan flags. These live on the path because stroke-generated
    /// outlines need one consistent fill rule across all flattened edges.
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

/// Keep horizontal edges that lie exactly on tile boundaries during scan conversion.
///
/// Normal filled paths skip those edges to avoid double ownership. Thin horizontal
/// strokes are first converted into very small filled outlines; if the boundary
/// edge is dropped but the opposite edge remains, backdrop fill can leak across
/// whole tiles and turn dashed strokes into blocks.
pub(crate) const PATH_FLAG_KEEP_HORIZONTAL_TILE_EDGES: u32 = 1 << 0;
