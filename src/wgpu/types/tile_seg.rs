use bytemuck::{Pod, Zeroable};

/// Tile-local line segment produced by scan_assign.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct TileSegment {
    pub path_ix: u32,
    pub tile_ix: u32,
    pub y_edge: f32,
    pub point0: [f32; 2],
    pub point1: [f32; 2],
}
