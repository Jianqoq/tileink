#[repr(C)]
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LineSegment {
    pub(crate) p0x: f32,
    pub(crate) p0y: f32,
    pub(crate) p1x: f32,
    pub(crate) p1y: f32,
    pub(crate) y_edge: f32,
}
