mod cpu;
mod render;
mod scene;
mod shared;
mod wgpu;

pub const TILE_SIZE: u32 = 16;
pub const TILE_SCALE: f32 = 1.0 / TILE_SIZE as f32;
pub const BLOCK_SIZE: u32 = 16 * 16;

pub use cpu::Renderer as CpuRenderer;
pub use scene::Scene;
pub use shared::{
    brush::Brush,
    fill::FillRule,
    image::Image,
    layer::{filter::Filter, region::Region},
    sdf::rect::Radius,
};
pub use wgpu::Renderer as WgpuRenderer;
pub use wgpu::WgpuBufferLengths;
