use crate::shared::{brush::Brush, pixel::premul_f32_to_u32};

pub const BRUSH_KIND_SOLID: u32 = 0;
pub const BRUSH_KIND_LINEAR: u32 = 1;
pub const BRUSH_KIND_RADIAL: u32 = 2;
pub const BRUSH_KIND_SWEEP: u32 = 3;
pub const BRUSH_KIND_FOUR_CORNER: u32 = 4;
pub const BRUSH_KIND_PATTERN: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuBrush {
    pub kind: u32,
    pub ramp_off: u32,
    pub ramp_len: u32,
    pub start_x: f32,
    pub start_y: f32,
    pub end_x: f32,
    pub end_y: f32,
    pub param0: f32,
    pub param1: f32,
    pub extend: u32,
    pub color0: u32,
    pub color1: u32,
    pub color2: u32,
    pub color3: u32,
    pub transform_sx: f32,
    pub transform_ky: f32,
    pub transform_kx: f32,
    pub transform_sy: f32,
    pub transform_tx: f32,
    pub transform_ty: f32,
}

impl GpuBrush {
    pub fn from_brush(brush: &Brush, ramp_off: u32, ramp_len: u32) -> Self {
        match brush {
            Brush::Solid(color) => Self {
                kind: BRUSH_KIND_SOLID,
                color0: premul_f32_to_u32(color.premultiply().components),
                ..Self::default()
            },
            Brush::Linear(gradient) => Self {
                kind: BRUSH_KIND_LINEAR,
                ramp_off,
                ramp_len,
                start_x: gradient.start[0],
                start_y: gradient.start[1],
                end_x: gradient.end[0],
                end_y: gradient.end[1],
                extend: gradient.extend as u32,
                ..Self::default()
            },
            Brush::Radial(gradient) => Self {
                kind: BRUSH_KIND_RADIAL,
                ramp_off,
                ramp_len,
                start_x: gradient.start_center[0],
                start_y: gradient.start_center[1],
                end_x: gradient.end_center[0],
                end_y: gradient.end_center[1],
                param0: gradient.start_radius,
                param1: gradient.end_radius,
                extend: gradient.extend as u32,
                transform_sx: gradient.transform[0],
                transform_ky: gradient.transform[1],
                transform_kx: gradient.transform[2],
                transform_sy: gradient.transform[3],
                transform_tx: gradient.transform[4],
                transform_ty: gradient.transform[5],
                ..Self::default()
            },
            Brush::Sweep(gradient) => Self {
                kind: BRUSH_KIND_SWEEP,
                ramp_off,
                ramp_len,
                start_x: gradient.center[0],
                start_y: gradient.center[1],
                param0: gradient.start_angle,
                param1: gradient.end_angle,
                extend: gradient.extend as u32,
                ..Self::default()
            },
            Brush::FourCorner(gradient) => Self {
                kind: BRUSH_KIND_FOUR_CORNER,
                start_x: gradient.bounds[0],
                start_y: gradient.bounds[1],
                end_x: gradient.bounds[2],
                end_y: gradient.bounds[3],
                color0: gradient.colors[0],
                color1: gradient.colors[1],
                color2: gradient.colors[2],
                color3: gradient.colors[3],
                ..Self::default()
            },
            Brush::Pattern(pattern) => Self {
                kind: BRUSH_KIND_PATTERN,
                ramp_off,
                ramp_len,
                param0: pattern.opacity as f32 / 255.0,
                color0: pattern.image.width,
                color1: pattern.image.height,
                transform_sx: pattern.transform[0],
                transform_ky: pattern.transform[1],
                transform_kx: pattern.transform[2],
                transform_sy: pattern.transform[3],
                transform_tx: pattern.transform[4],
                transform_ty: pattern.transform[5],
                ..Self::default()
            },
        }
    }
}
