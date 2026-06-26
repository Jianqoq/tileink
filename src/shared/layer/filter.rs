use peniko::{Color, kurbo};

use crate::shared::{
    bounds::Bounds,
    brush::Brush,
    image::Image,
    layer::{region::Region, blend::Blend, mask::MaskMode},
    pixel::{pack_premul_rgba8, src_over, unpack_premul_rgba8},
};

#[derive(Clone, Debug)]
pub enum Filter {
    Blur(f32),
    Brightness(f32),
    Contrast(f32),
    Grayscale(f32),
    HueRotate(f32),
    Invert(f32),
    Opacity(f32),
    Saturate(f32),
    Sepia(f32),
    DropShadow {
        offset_x: f32,
        offset_y: f32,
        radius: f32,
        brush: Brush,
    },
}
