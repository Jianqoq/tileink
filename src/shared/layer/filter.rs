use peniko::Mix;

use crate::shared::bounds::Bounds;
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
    /// A lowered SVG filter graph. Primitive regions are absolute pixel bounds;
    /// each primitive output is transparent outside its own region.
    Graph {
        primitives: Vec<FilterPrimitive>,
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
    Morphology {
        radius_x: f32,
        radius_y: f32,
        operator: MorphologyOperator,
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

#[derive(Clone, Debug)]
pub struct FilterPrimitive {
    pub input: FilterInput,
    pub input2: Option<FilterInput>,
    pub region: Bounds,
    pub kind: FilterPrimitiveKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterInput {
    SourceGraphic,
    SourceAlpha,
    Primitive(usize),
}

#[derive(Clone, Debug)]
pub enum FilterPrimitiveKind {
    Identity,
    Filter(Box<Filter>),
    Blend { mode: Mix },
    Composite { operator: CompositeOperator },
    Merge { inputs: Vec<FilterInput> },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CompositeOperator {
    Over,
    In,
    Out,
    Atop,
    Xor,
    Arithmetic { k1: f32, k2: f32, k3: f32, k4: f32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MorphologyOperator {
    Erode,
    Dilate,
}
