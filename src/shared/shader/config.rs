/// Stable native shader ABI, bound at Metal buffer(0) / HLSL b0 (Vulkan binding 0).
///
/// Scalars appear in this exact order, four bytes each. Input texture is slot 1,
/// RGBA8 output texture slot 3, compact tile IDs slot 8, raw uniform words slot 10.
/// Each 256-lane workgroup uses `group.x + group.y * dispatch_width` as its
/// linear group index. Compact tiles are 16x16; shaders must check both the
/// region and canvas bounds before reading/writing. Output coordinates subtract
/// `output_x0/output_y0`; logical positions apply the inverse affine to p+0.5.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShaderConfig {
    pub width: u32,
    pub height: u32,
    pub region_x0: u32,
    pub region_y0: u32,
    pub region_width: u32,
    pub region_height: u32,
    pub dispatch_width: u32,
    pub pixel_count: u32,
    pub compact_tiles: u32,
    pub active_tile_count: u32,
    pub tiles_width: u32,
    pub tiles_height: u32,
    pub inverse_xx: f32,
    pub inverse_yx: f32,
    pub inverse_xy: f32,
    pub inverse_yy: f32,
    pub inverse_tx: f32,
    pub inverse_ty: f32,
    pub sample_xx: f32,
    pub sample_yx: f32,
    pub sample_xy: f32,
    pub sample_yy: f32,
    pub sample_radius: f32,
    pub uniform_words: u32,
    pub output_x0: u32,
    pub output_y0: u32,
    pub output_width: u32,
    pub output_height: u32,
}
