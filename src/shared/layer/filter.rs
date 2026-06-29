use peniko::{Mix, kurbo::Shape};

use crate::shared::bounds::Bounds;
use crate::shared::brush::Brush;
use crate::shared::layer::region::Region;

pub const COMPONENT_TRANSFER_TABLE_SIZE: usize = 256;
pub const COMPONENT_TRANSFER_CHANNELS: usize = 4;
pub const COMPONENT_TRANSFER_TABLE_LEN: usize =
    COMPONENT_TRANSFER_TABLE_SIZE * COMPONENT_TRANSFER_CHANNELS;
pub const TURBULENCE_LATTICE_SIZE: usize = 256;
pub const TURBULENCE_TABLE_LEN: usize = TURBULENCE_LATTICE_SIZE * 2 + 2;
pub const TURBULENCE_CHANNELS: usize = 4;
pub const TURBULENCE_GRADIENT_COMPONENTS: usize = 2;
pub const TURBULENCE_GRADIENT_LEN: usize =
    TURBULENCE_CHANNELS * TURBULENCE_TABLE_LEN * TURBULENCE_GRADIENT_COMPONENTS;
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
    Blur {
        radius_x: f32,
        radius_y: f32,
    },
    Brightness(f32),
    Contrast(f32),
    ColorMatrix([f32; 20]),
    ComponentTransfer(Box<ComponentTransferTable>),
    ConvolveMatrix(ConvolveMatrix),
    DiffuseLighting(DiffuseLighting),
    SpecularLighting(SpecularLighting),
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
    /// Produces an image without reading a graph input, matching SVG `feImage`.
    ///
    /// The brush is sampled in absolute scene coordinates and the primitive
    /// region clips the output, so later graph primitives can consume it like
    /// any other filter result.
    Image {
        brush: Brush,
    },
    Blend {
        mode: Mix,
    },
    Composite {
        operator: CompositeOperator,
    },
    /// Samples the first input at offsets derived from the second input's color channels.
    ///
    /// The displacement map reads unpremultiplied channel values; RGB channels
    /// are converted back to linear values when the SVG primitive uses the
    /// default `color-interpolation-filters="linearRGB"` space.
    DisplacementMap(DisplacementMap),
    /// Repeats an input result over this primitive's region, matching SVG `feTile`.
    ///
    /// `source_region` is the tile cell in absolute pixel coordinates after
    /// lowering prior primitive effects such as `feOffset`.
    Tile {
        source_region: Bounds,
    },
    /// Generates an RGBA noise image without reading graph inputs, matching SVG `feTurbulence`.
    Turbulence(Turbulence),
    Merge {
        inputs: Vec<FilterInput>,
    },
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplacementMap {
    pub scale_x: f32,
    pub scale_y: f32,
    pub x_channel: ColorChannel,
    pub y_channel: ColorChannel,
    pub linear_rgb: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorChannel {
    R,
    G,
    B,
    A,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turbulence {
    pub base_frequency_x: f32,
    pub base_frequency_y: f32,
    pub num_octaves: u32,
    pub seed: i32,
    pub stitch_tiles: bool,
    pub kind: TurbulenceKind,
    pub linear_rgb: bool,
    pub transform_x: f32,
    pub transform_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub tile_x: f32,
    pub tile_y: f32,
    pub tile_width: f32,
    pub tile_height: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurbulenceKind {
    Turbulence,
    FractalNoise,
}

#[derive(Clone, Debug)]
pub(crate) struct TurbulenceLattice {
    pub(crate) selectors: [u32; TURBULENCE_TABLE_LEN],
    pub(crate) gradients: Vec<f32>,
}

pub(crate) fn turbulence_lattice(seed: i32) -> TurbulenceLattice {
    const RAND_M: i64 = 2_147_483_647;
    const RAND_A: i64 = 16_807;
    const RAND_Q: i64 = 127_773;
    const RAND_R: i64 = 2_836;

    fn setup_seed(seed: i32) -> i64 {
        let mut seed = i64::from(seed);
        if seed <= 0 {
            seed = -(seed % (RAND_M - 1)) + 1;
        }
        seed.min(RAND_M - 1)
    }

    fn next_random(seed: &mut i64) -> i64 {
        let result = RAND_A * (*seed % RAND_Q) - RAND_R * (*seed / RAND_Q);
        *seed = if result <= 0 { result + RAND_M } else { result };
        *seed
    }

    let mut seed = setup_seed(seed);
    let mut selectors = [0; TURBULENCE_TABLE_LEN];
    let mut gradients = vec![0.0; TURBULENCE_GRADIENT_LEN];
    for channel in 0..TURBULENCE_CHANNELS {
        for (i, selector) in selectors
            .iter_mut()
            .take(TURBULENCE_LATTICE_SIZE)
            .enumerate()
        {
            *selector = i as u32;
            let gx = (next_random(&mut seed) % (TURBULENCE_LATTICE_SIZE * 2) as i64) as f32
                - TURBULENCE_LATTICE_SIZE as f32;
            let gy = (next_random(&mut seed) % (TURBULENCE_LATTICE_SIZE * 2) as i64) as f32
                - TURBULENCE_LATTICE_SIZE as f32;
            let len = (gx * gx + gy * gy).sqrt();
            let base = turbulence_gradient_index(channel, i);
            if len > f32::EPSILON {
                gradients[base] = gx / len;
                gradients[base + 1] = gy / len;
            }
        }
    }

    for i in (1..TURBULENCE_LATTICE_SIZE).rev() {
        let j = (next_random(&mut seed) % TURBULENCE_LATTICE_SIZE as i64) as usize;
        selectors.swap(i, j);
    }

    for i in 0..(TURBULENCE_LATTICE_SIZE + 2) {
        selectors[TURBULENCE_LATTICE_SIZE + i] = selectors[i];
        for channel in 0..TURBULENCE_CHANNELS {
            let dst = turbulence_gradient_index(channel, TURBULENCE_LATTICE_SIZE + i);
            let src = turbulence_gradient_index(channel, i);
            gradients[dst] = gradients[src];
            gradients[dst + 1] = gradients[src + 1];
        }
    }

    TurbulenceLattice {
        selectors,
        gradients,
    }
}

#[inline]
pub(crate) fn turbulence_gradient_index(channel: usize, selector: usize) -> usize {
    (channel * TURBULENCE_TABLE_LEN + selector) * TURBULENCE_GRADIENT_COMPONENTS
}

pub(crate) fn filter_offset_to_pixel_delta(delta: f32) -> i32 {
    // Filter buffers are sampled at pixel centers. A positive N+0.5 offset moves
    // the source edge onto the next pixel center, so that pixel still owns the edge.
    (delta - 0.5).ceil() as i32
}

pub(crate) fn filtered_region_bounds(
    filter: &Filter,
    sample_region: &Region,
    canvas: Bounds,
) -> Bounds {
    unclipped_filtered_region_bounds(filter, sample_region).intersect(canvas)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FilterSurfaceBounds {
    pub(crate) surface: Bounds,
    pub(crate) output: Bounds,
}

pub(crate) fn filter_surface_bounds(
    filter: &Filter,
    sample_region: &Region,
    target_bounds: Bounds,
) -> Option<FilterSurfaceBounds> {
    let full_output = unclipped_filtered_region_bounds(filter, sample_region);
    let output = full_output.intersect(target_bounds);
    if output.is_empty() {
        return None;
    }

    // A filter can read source pixels outside its final output through blur,
    // morphology, offset, drop-shadow, or graph tiling. The surface keeps only
    // pixels that can still influence the visible target, so very large source
    // bboxes outside the canvas do not turn into huge intermediate buffers.
    let source_window = target_bounds.outset(filter_dependency_outset(filter));
    let surface = full_output.intersect(source_window);
    (!surface.is_empty()).then_some(FilterSurfaceBounds { surface, output })
}

pub(crate) fn unclipped_filtered_region_bounds(filter: &Filter, sample_region: &Region) -> Bounds {
    region_bounds(sample_region).outset(filter_outset(filter))
}

fn filter_outset(filter: &Filter) -> i32 {
    match filter {
        Filter::Chain {
            filters,
            fixed_region,
        } => {
            if *fixed_region {
                0
            } else {
                filters.iter().map(filter_outset).sum()
            }
        }
        Filter::Graph { .. } => 0,
        Filter::Blur { radius_x, radius_y } => blur_outset(radius_x.max(*radius_y)),
        Filter::Offset { dx, dy } => dx.abs().ceil().max(dy.abs().ceil()) as i32,
        Filter::Morphology {
            radius_x,
            radius_y,
            operator,
        } => match operator {
            MorphologyOperator::Erode => 0,
            MorphologyOperator::Dilate => (*radius_x).max(*radius_y).max(0.0).ceil() as i32,
        },
        Filter::DropShadow {
            radius,
            offset_x,
            offset_y,
            ..
        } => blur_outset(*radius) + offset_x.abs().ceil().max(offset_y.abs().ceil()) as i32,
        _ => 0,
    }
}

fn filter_dependency_outset(filter: &Filter) -> i32 {
    match filter {
        Filter::Chain { filters, .. } => filters.iter().map(filter_dependency_outset).sum(),
        Filter::Graph { primitives, .. } => graph_dependency_outset(primitives),
        _ => filter_outset(filter),
    }
}

fn graph_dependency_outset(primitives: &[FilterPrimitive]) -> i32 {
    primitives.iter().map(primitive_dependency_outset).sum()
}

fn primitive_dependency_outset(primitive: &FilterPrimitive) -> i32 {
    match &primitive.kind {
        FilterPrimitiveKind::Filter(filter) => filter_dependency_outset(filter),
        FilterPrimitiveKind::DisplacementMap(map) => {
            (map.scale_x.abs().max(map.scale_y.abs()) * 0.5).ceil() as i32
        }
        FilterPrimitiveKind::Tile { source_region } => {
            bounds_distance(primitive.region, *source_region)
        }
        FilterPrimitiveKind::Identity
        | FilterPrimitiveKind::Blend { .. }
        | FilterPrimitiveKind::Composite { .. }
        | FilterPrimitiveKind::Image { .. }
        | FilterPrimitiveKind::Merge { .. } => 0,
        FilterPrimitiveKind::Turbulence(_) => 0,
    }
}

fn bounds_distance(a: Bounds, b: Bounds) -> i32 {
    (a.x0 - b.x0)
        .abs()
        .max((a.y0 - b.y0).abs())
        .max((a.x1 - b.x1).abs())
        .max((a.y1 - b.y1).abs())
}

fn region_bounds(region: &Region) -> Bounds {
    match region {
        Region::Rect { rect, .. } => rect_bounds(*rect),
        Region::Path {
            path,
            transform,
            tolerance: _,
        } => {
            let path = *transform * path;
            rect_bounds(path.bounding_box())
        }
    }
}

fn rect_bounds(rect: peniko::kurbo::Rect) -> Bounds {
    Bounds::new(
        rect.x0.floor() as i32,
        rect.y0.floor() as i32,
        rect.x1.ceil() as i32,
        rect.y1.ceil() as i32,
    )
}

fn blur_outset(radius: f32) -> i32 {
    (radius.max(0.0) * 3.0).ceil() as i32
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
pub struct SpecularLighting {
    pub surface_scale: f32,
    pub specular_constant: f32,
    pub specular_exponent: f32,
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
