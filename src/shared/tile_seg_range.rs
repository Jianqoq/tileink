#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TileSegmentRange {
    pub start: u32,
    pub end: u32, // exclusive
}
