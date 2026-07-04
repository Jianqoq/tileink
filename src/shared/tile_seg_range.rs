#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TileSegmentRange {
    pub start: u32,
    pub end: u32, // exclusive
}
