#[derive(Debug, Clone, Copy)]
pub struct LineSegment {
    pub(crate) point0: (f32, f32),
    pub(crate) point1: (f32, f32),
    pub(crate) y_edge: f32,
}
