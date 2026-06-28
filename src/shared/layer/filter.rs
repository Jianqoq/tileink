use crate::shared::brush::Brush;

pub const COMPONENT_TRANSFER_TABLE_SIZE: usize = 256;
pub const COMPONENT_TRANSFER_CHANNELS: usize = 4;
pub const COMPONENT_TRANSFER_TABLE_LEN: usize =
    COMPONENT_TRANSFER_TABLE_SIZE * COMPONENT_TRANSFER_CHANNELS;
/// Fixed RGBA lookup table for SVG `feComponentTransfer`.
///
/// Each channel owns 256 u32 entries in R, G, B, A order. Values are stored as
/// 0..255 bytes so CPU and CubeCL can share the same quantized semantics.
pub type ComponentTransferTable = [u32; COMPONENT_TRANSFER_TABLE_LEN];

#[derive(Clone, Debug)]
pub enum Filter {
    Chain {
        filters: Vec<Filter>,
        fixed_region: bool,
    },
    Blur(f32),
    Brightness(f32),
    Contrast(f32),
    ColorMatrix([f32; 20]),
    ComponentTransfer(Box<ComponentTransferTable>),
    Flood {
        brush: Brush,
    },
    Grayscale(f32),
    HueRotate(f32),
    Invert(f32),
    Offset {
        dx: f32,
        dy: f32,
    },
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
