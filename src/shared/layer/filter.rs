use peniko::{Color, Mix, kurbo::Shape};

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
/// 0..255 bytes so CPU and wgpu can share the same quantized semantics.
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
    /// Custom rounded-rectangle liquid-glass backdrop filter.
    ///
    /// The effect samples the already-rendered backdrop, applies an internal
    /// blurred backdrop copy, then refracts/tints/highlights pixels from the
    /// rectangular filter region edge. It is designed for
    /// `Scene::push_backdrop_layer` with `Region::Rect`; path regions are
    /// rejected because the refraction model depends on rounded-rectangle SDF
    /// normals. Shadow is intentionally not part of this filter; draw a
    /// separate SDF rectangle shadow before the backdrop layer when needed.
    RectLiquidGlass(RectLiquidGlass),
    Blur {
        /// Gaussian standard deviation in pixels on the X axis.
        std_dev_x: f32,
        /// Gaussian standard deviation in pixels on the Y axis.
        std_dev_y: f32,
        /// Optional reduced-resolution sampling strategy for large blurs.
        sampling: BlurSampling,
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
        /// Gaussian standard deviation in pixels for the shadow alpha blur.
        std_dev: f32,
        brush: Brush,
    },
}

impl Filter {
    pub(crate) fn contains_rect_liquid_glass(&self) -> bool {
        match self {
            Self::RectLiquidGlass(_) => true,
            Self::Chain { filters, .. } => filters.iter().any(Self::contains_rect_liquid_glass),
            Self::Graph { primitives, .. } => primitives
                .iter()
                .any(|primitive| primitive.kind.contains_rect_liquid_glass()),
            _ => false,
        }
    }
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

impl FilterPrimitiveKind {
    fn contains_rect_liquid_glass(&self) -> bool {
        match self {
            Self::Filter(filter) => filter.contains_rect_liquid_glass(),
            _ => false,
        }
    }
}

pub(crate) const LIQUID_GLASS_BLUR_STD_DEV_SCALE: f32 = 1.0 / 3.0;
pub(crate) const LIQUID_GLASS_CHROMATIC_R: f32 = 0.98;
pub(crate) const LIQUID_GLASS_CHROMATIC_G: f32 = 1.0;
pub(crate) const LIQUID_GLASS_CHROMATIC_B: f32 = 1.02;
pub(crate) const LIQUID_GLASS_REFRACTION_PIXEL_SCALE: f32 = std::f32::consts::SQRT_2 * 50.0;
pub(crate) const LIQUID_GLASS_NORMAL_LENGTH_SCALE: f32 = std::f32::consts::SQRT_2 * 1000.0;
pub(crate) const LIQUID_GLASS_ACTIVE_DISTANCE_NORM: f32 = 0.005;
pub(crate) const LIQUID_GLASS_EDGE_BLEND_START: f32 = -0.001;
pub(crate) const LIQUID_GLASS_EDGE_BLEND_END: f32 = 0.001;
pub(crate) const LIQUID_GLASS_TINT_MIX: f32 = 0.8;
pub(crate) const LIQUID_GLASS_TINT_BASE_MIX: f32 = 0.5;
pub(crate) const LIQUID_GLASS_FRESNEL_LIGHTNESS_GAIN: f32 = 20.0;
pub(crate) const LIQUID_GLASS_FRESNEL_MIX_SCALE: f32 = 0.7;
pub(crate) const LIQUID_GLASS_GLARE_LIGHTNESS_GAIN: f32 = 150.0;
pub(crate) const LIQUID_GLASS_GLARE_CHROMA_GAIN: f32 = 30.0;
pub(crate) const LIQUID_GLASS_GLARE_SIDE_SCALE: f32 = 1.2;
pub(crate) const LIQUID_GLASS_GLARE_POWER_BASE: f32 = 0.1;
pub(crate) const LIQUID_GLASS_GLARE_POWER_SCALE: f32 = 2.0;
pub(crate) const LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE: f32 = 1500.0;
pub(crate) const LIQUID_GLASS_GEOMETRY_RANGE_SCALE: f32 = 500.0;
pub(crate) const LIQUID_GLASS_EPSILON: f32 = 1e-6;
pub(crate) const LIQUID_GLASS_D65_X: f32 = 0.9504559;
pub(crate) const LIQUID_GLASS_D65_Y: f32 = 1.0;
pub(crate) const LIQUID_GLASS_D65_Z: f32 = 1.0890578;
pub(crate) const LIQUID_GLASS_D65_WHITE: [f32; 3] =
    [LIQUID_GLASS_D65_X, LIQUID_GLASS_D65_Y, LIQUID_GLASS_D65_Z];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlurDownsampleFilter {
    /// Sample one source pixel per low-resolution pixel. Fastest, lowest quality.
    Nearest,
    /// Average every covered source pixel in the low-resolution cell. This is
    /// the default because it preserves energy before the Gaussian blur pass.
    Box,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlurUpsampleFilter {
    /// Copy the nearest low-resolution pixel back to full resolution.
    Nearest,
    /// Bilinearly interpolate premultiplied RGBA. This is the default quality
    /// choice for backdrop blur because it avoids block edges after upsample.
    Bilinear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlurSampling {
    /// `1` keeps full-resolution blur. Larger values downsample, blur at
    /// reduced resolution, then upsample to the original filter bounds.
    pub factor: u32,
    pub downsample_filter: BlurDownsampleFilter,
    pub upsample_filter: BlurUpsampleFilter,
}

impl BlurSampling {
    pub const FULL_RES: Self = Self {
        factor: 1,
        downsample_filter: BlurDownsampleFilter::Box,
        upsample_filter: BlurUpsampleFilter::Bilinear,
    };

    pub fn downsampled(factor: u32) -> Self {
        Self {
            factor,
            ..Self::FULL_RES
        }
    }

    pub(crate) fn factor(self) -> u32 {
        self.factor.max(1)
    }
}

impl Default for BlurSampling {
    fn default() -> Self {
        Self::FULL_RES
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectLiquidGlass {
    /// Effect controls follow liquid-glass-studio's public UI values. Percent-like
    /// controls such as `fresnel_factor` and `glare_hardness` stay in 0..100 here
    /// and are normalized only at the CPU/GPU pass boundary.
    /// Reference blur kernel radius in pixels. Internally this maps to
    /// Gaussian `std_dev = blur_radius / 3`, matching liquid-glass-studio.
    pub blur_radius: u32,
    /// Sampling strategy for the internal blurred backdrop.
    pub blur_sampling: BlurSampling,
    pub blur_edge: bool,
    pub tint: Color,
    pub refraction_thickness: f32,
    pub refraction_factor: f32,
    pub refraction_dispersion: f32,
    pub fresnel_range: f32,
    pub fresnel_hardness: f32,
    pub fresnel_factor: f32,
    pub glare_range: f32,
    pub glare_hardness: f32,
    pub glare_convergence: f32,
    pub glare_opposite_factor: f32,
    pub glare_factor: f32,
    pub glare_angle: f32,
}

impl Default for RectLiquidGlass {
    fn default() -> Self {
        Self {
            blur_radius: 1,
            blur_sampling: BlurSampling::default(),
            blur_edge: true,
            tint: Color::from_rgba8(255, 255, 255, 0),
            refraction_thickness: 20.0,
            refraction_factor: 1.4,
            refraction_dispersion: 7.0,
            fresnel_range: 30.0,
            fresnel_hardness: 20.0,
            fresnel_factor: 20.0,
            glare_range: 30.0,
            glare_hardness: 20.0,
            glare_convergence: 50.0,
            glare_opposite_factor: 80.0,
            glare_factor: 90.0,
            glare_angle: -45.0_f32.to_radians(),
        }
    }
}

impl RectLiquidGlass {
    pub(crate) fn sample_outset(self) -> i32 {
        let refraction = LIQUID_GLASS_REFRACTION_PIXEL_SCALE.ceil() as i32;
        blur_outset(self.blur_radius as f32 * LIQUID_GLASS_BLUR_STD_DEV_SCALE) + refraction + 2
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RectLiquidGlassRegion {
    pub(crate) x0: f32,
    pub(crate) y0: f32,
    pub(crate) x1: f32,
    pub(crate) y1: f32,
    pub(crate) radius_top_left: f32,
    pub(crate) radius_top_right: f32,
    pub(crate) radius_bottom_left: f32,
    pub(crate) radius_bottom_right: f32,
}

impl RectLiquidGlassRegion {
    fn from_bounds(bounds: Bounds) -> Self {
        Self {
            x0: bounds.x0 as f32,
            y0: bounds.y0 as f32,
            x1: bounds.x1 as f32,
            y1: bounds.y1 as f32,
            radius_top_left: 0.0,
            radius_top_right: 0.0,
            radius_bottom_left: 0.0,
            radius_bottom_right: 0.0,
        }
    }
}

pub(crate) fn rect_liquid_glass_region(
    region: Option<&crate::shared::layer::region::Region>,
    fallback_bounds: Bounds,
) -> RectLiquidGlassRegion {
    match region {
        Some(crate::shared::layer::region::Region::Rect { rect, radius }) => {
            let x0 = rect.x0.min(rect.x1) as f32;
            let y0 = rect.y0.min(rect.y1) as f32;
            let x1 = rect.x0.max(rect.x1) as f32;
            let y1 = rect.y0.max(rect.y1) as f32;
            RectLiquidGlassRegion {
                x0,
                y0,
                x1,
                y1,
                radius_top_left: radius.top_left,
                radius_top_right: radius.top_right,
                radius_bottom_left: radius.bottom_left,
                radius_bottom_right: radius.bottom_right,
            }
        }
        Some(crate::shared::layer::region::Region::Path { .. }) => {
            panic!(
                "RectLiquidGlass requires Region::Rect because it uses rounded-rectangle SDF normals"
            )
        }
        None => RectLiquidGlassRegion::from_bounds(fallback_bounds),
    }
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
        Filter::RectLiquidGlass(glass) => glass.sample_outset(),
        Filter::Blur {
            std_dev_x,
            std_dev_y,
            sampling: _,
        } => blur_outset(std_dev_x.max(*std_dev_y)),
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
            std_dev,
            offset_x,
            offset_y,
            ..
        } => blur_outset(*std_dev) + offset_x.abs().ceil().max(offset_y.abs().ceil()) as i32,
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

fn blur_outset(std_dev: f32) -> i32 {
    (std_dev.max(0.0) * 3.0).ceil() as i32
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
