#[derive(Debug, Clone, Copy, Default)]
pub struct LineSegment {
    pub(crate) path_id: u32,
    pub(crate) tile_id: u32,
    pub(crate) point0: (f32, f32),
    pub(crate) point1: (f32, f32),
    pub(crate) y_edge: f32,
    pub(crate) coverages: [Coverage; 32]
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Coverage {
    pub(crate) x: u8,
    pub(crate) y: u8,
    pub(crate) coverage: u8,
}