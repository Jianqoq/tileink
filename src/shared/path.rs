/// Per-path slice into the scene `path_data` byte stream.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PathRecord {
    pub path_id: u32,
    pub line_count: u32,
    pub line_start: u32,
    pub _pad: u32,
}
