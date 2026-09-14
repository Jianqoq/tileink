use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use crate::shared::filter_config::FilterConfig;

#[path = "filter/inputs.rs"]
pub mod inputs;
#[path = "filter/morphology.rs"]
pub mod morphology;
#[path = "filter/region.rs"]
mod region;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasicFilter {
    Clear,
    Copy,
    SourceAlpha,
    SourceOver,
    SvgMask,
    Color,
    ColorMatrix,
    Tile,
    Offset,
    DropShadowMask,
}
impl BasicFilter {
    pub fn reads_source(self) -> bool {
        !matches!(self, Self::Clear | Self::Color | Self::ColorMatrix)
    }
    pub fn entry(self) -> &'static str {
        match self {
            Self::Clear => "filter_clear_region",
            Self::Copy => "filter_copy_region",
            Self::SourceOver => "filter_source_over_region",
            Self::Color => "filter_color_region",
            Self::ColorMatrix => "filter_color_matrix_region",
            Self::SvgMask => "filter_svg_mask_coverage_region",
            Self::SourceAlpha => "filter_source_alpha_region",
            Self::Tile => "filter_tile_region",
            Self::Offset => "filter_offset_region",
            Self::DropShadowMask => "filter_drop_shadow_mask_region",
        }
    }
}

/// Validate logical coordinates and unique write ownership before recording raw native work.
/// Dispatch/list counts are derived here, so stale physical capacity never becomes live pixels.
pub fn encode(
    batch: &mut ComputeBatch,
    kernel: BasicFilter,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    source: Option<ResourceId>,
    target: ResourceId,
) -> Result<()> {
    let source = if kernel.reads_source() {
        Some(source.ok_or("filter source texture is required")?)
    } else {
        None
    };
    for (extent, offset) in [
        (config.width, config.offset_x),
        (config.height, config.offset_y),
    ] {
        // Both addition (shadow) and subtraction (offset) must fit the shader's signed coordinates.
        if i64::from(extent) + i64::from(offset).abs() > i64::from(i32::MAX) {
            return Err("filter signed coordinate overflow".into());
        }
    }
    if kernel == BasicFilter::Color && !config.amount.is_finite() {
        return Err("nonfinite filter color amount".into());
    }
    if kernel == BasicFilter::ColorMatrix
        && [
            config.matrix_r,
            config.matrix_g,
            config.matrix_b,
            config.matrix_a,
            config.matrix_bias,
        ]
        .iter()
        .flatten()
        .any(|v| !v.is_finite())
    {
        return Err("nonfinite filter color matrix".into());
    }
    if kernel == BasicFilter::Tile {
        let rect = [
            config.rect_x0,
            config.rect_y0,
            config.rect_x1,
            config.rect_y1,
        ];
        if rect.iter().any(|v| !v.is_finite() || *v < 0.0)
            || config.rect_x0 > config.rect_x1
            || config.rect_y0 > config.rect_y1
            || f64::from(config.rect_x1) > f64::from(config.width)
            || f64::from(config.rect_y1) > f64::from(config.height)
        {
            return Err("invalid filter tile source rectangle".into());
        }
    }
    let reads = source.map(|id| (1, id));
    region::record(
        batch,
        kernel.entry(),
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings::textures(reads.as_slice(), [config.width, config.height]),
        target,
    )
}

#[path = "filter/displacement.rs"]
pub mod displacement;

#[path = "filter/transfer.rs"]
pub mod transfer;

#[path = "filter/convolve.rs"]
pub mod convolve;

#[path = "filter/resample.rs"]
pub mod resample;

#[path = "filter/blur.rs"]
pub mod blur;

#[path = "filter/lighting.rs"]
pub mod lighting;

#[path = "filter/rectangle.rs"]
pub mod rectangle;

#[path = "filter/path_mask.rs"]
pub mod path_mask;

#[path = "filter/surface.rs"]
pub mod surface;
#[path = "filter/turbulence.rs"]
pub mod turbulence;

#[path = "filter/layer.rs"]
pub mod layer;

#[path = "filter/stack.rs"]
pub mod stack;

#[path = "filter/glass.rs"]
pub mod glass;
