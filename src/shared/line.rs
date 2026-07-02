/// One flattenable primitive (line / quad / cubic / close) for parallel CPU flatten.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Line {
    pub path_id: u32,
    pub flags: f32,
    pub p0: [f32; 2],
    pub p1: [f32; 2],
}

pub(crate) const LINE_FLAG_KEEP_HORIZONTAL_TILE_EDGES: f32 = 1.0;
