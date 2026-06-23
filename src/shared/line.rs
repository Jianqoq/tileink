/// One flattenable primitive (line / quad / cubic / close) for parallel CPU flatten.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Line {
    pub path_id: u32,
    pub _pad: f32,
    pub p0: [f32; 2],
    pub p1: [f32; 2],
}
