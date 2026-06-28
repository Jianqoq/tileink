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
    ConvolveMatrix(ConvolveMatrix),
    DiffuseLighting(DiffuseLighting),
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

#[derive(Clone, Debug)]
pub struct ConvolveMatrix {
    pub columns: u32,
    pub rows: u32,
    pub target_x: u32,
    pub target_y: u32,
    pub data: Vec<f32>,
    pub divisor: f32,
    pub bias: f32,
    pub edge_mode: ConvolveEdgeMode,
    pub preserve_alpha: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConvolveEdgeMode {
    None,
    Duplicate,
    Wrap,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffuseLighting {
    pub surface_scale: f32,
    pub diffuse_constant: f32,
    pub lighting_color: [f32; 3],
    pub light_source: LightSource,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightSource {
    Distant {
        azimuth: f32,
        elevation: f32,
    },
    Point {
        x: f32,
        y: f32,
        z: f32,
    },
    Spot {
        x: f32,
        y: f32,
        z: f32,
        points_at_x: f32,
        points_at_y: f32,
        points_at_z: f32,
        specular_exponent: f32,
        limiting_cone_angle: Option<f32>,
    },
}
