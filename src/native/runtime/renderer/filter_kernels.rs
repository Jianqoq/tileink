use super::filter_encoding::FilterEncoding;
use super::*;
use crate::native::runtime::program::filter::{
    self, BasicFilter, blur, convolve, displacement, glass, inputs, lighting, morphology,
    rectangle, resample, transfer, turbulence,
};
use crate::render::filter_program::{ColorFilterKind, FilterKernel};
use crate::shared::{filter_parameters::*, gpu_constants::*, layer::filter as model};

impl FilterEncoding<'_, '_> {
    pub(super) fn record(&mut self, kernel: FilterKernel<'_>) -> Result<()> {
        use FilterKernel::*;
        let e = &mut self.execution;
        let bounds = match &kernel {
            ClearRenderTarget { .. } => Bounds::canvas(e.targets.size().0, e.targets.size().1),
            BlurRegionPartialToTarget { output_bounds, .. } => *output_bounds,
            DownsampleRegion { low_bounds, .. } => *low_bounds,
            ClearRenderRegion { bounds, .. }
            | FloodRegionToTarget { bounds, .. }
            | BuildDropShadowMaskToTarget { bounds, .. }
            | SourceAlphaToTarget { bounds, .. }
            | SourceOverFilterInput { bounds, .. }
            | BlendFilterInputs { bounds, .. }
            | CompositeFilterInputs { bounds, .. }
            | DisplacementMapFilterInputs { bounds, .. }
            | TurbulenceToTarget { bounds, .. }
            | TileFilterInput { bounds, .. }
            | CompositeDropShadowToTarget { bounds, .. }
            | ApplyColorMatrixToTarget { bounds, .. }
            | ApplyComponentTransferToTarget { bounds, .. }
            | ConvolveMatrixToTarget { bounds, .. }
            | DiffuseLightingToTarget { bounds, .. }
            | SpecularLightingToTarget { bounds, .. }
            | LiquidGlassToTarget { bounds, .. }
            | BlurRegionToTarget { bounds, .. }
            | MorphologyAxisToTarget { bounds, .. }
            | OffsetRegionToTarget { bounds, .. }
            | CopyRegionToTarget { bounds, .. }
            | ApplyColorFilterToTarget { bounds, .. }
            | UpsampleRegion { bounds, .. }
            | UpsampleRectCompositeRegion { bounds, .. }
            | RectLiquidGlassCompositeRegion { bounds, .. } => *bounds,
        };
        let Some(mut c) = e.config(bounds) else {
            return Ok(());
        };
        let tiles = self.work.as_deref();
        let image = |target| e.targets.get(target).map(Surface::image);
        match kernel {
            ClearRenderTarget { target, color } | ClearRenderRegion { target, color, .. } => {
                c.clear_color = color;
                filter::encode(e.batch, BasicFilter::Clear, c, tiles, None, image(target)?)
            }
            FloodRegionToTarget {
                target,
                brush_offset,
                ..
            }
            | CompositeDropShadowToTarget {
                target,
                brush_offset,
                ..
            } => {
                let shadow = if let CompositeDropShadowToTarget { shadow, .. } = kernel {
                    Some(image(shadow)?)
                } else {
                    None
                };
                c.brush_offset = brush_offset;
                // SAFETY: FilterResources and Images were built from the same immutable upload.
                unsafe {
                    e.filters
                        .brushes
                        .as_ref()
                        .ok_or("native filter brushes missing")?
                        .encode(
                            e.batch,
                            c,
                            tiles,
                            e.images.textures(),
                            shadow,
                            image(target)?,
                        )
                }
            }
            BuildDropShadowMaskToTarget {
                source,
                target,
                dx,
                dy,
                ..
            }
            | OffsetRegionToTarget {
                source,
                target,
                dx,
                dy,
                ..
            } => {
                c.offset_x = dx;
                c.offset_y = dy;
                let mode = if matches!(kernel, BuildDropShadowMaskToTarget { .. }) {
                    BasicFilter::DropShadowMask
                } else {
                    BasicFilter::Offset
                };
                filter::encode(
                    e.batch,
                    mode,
                    c,
                    tiles,
                    Some(image(source)?),
                    image(target)?,
                )
            }
            SourceAlphaToTarget { source, target, .. }
            | SourceOverFilterInput { source, target, .. }
            | CopyRegionToTarget { source, target, .. } => {
                let mode = match kernel {
                    SourceAlphaToTarget { .. } => BasicFilter::SourceAlpha,
                    SourceOverFilterInput { .. } => BasicFilter::SourceOver,
                    _ => BasicFilter::Copy,
                };
                filter::encode(
                    e.batch,
                    mode,
                    c,
                    tiles,
                    Some(image(source)?),
                    image(target)?,
                )
            }
            BlendFilterInputs {
                input1,
                input2,
                target,
                mode,
                ..
            } => {
                c.blend_mode = mode as u32 | ((peniko::Compose::SrcOver as u32) << 8);
                inputs::encode(
                    e.batch,
                    inputs::InputFilter::Blend {
                        source: image(input1)?,
                        backdrop: image(input2)?,
                    },
                    c,
                    tiles,
                    image(target)?,
                )
            }
            CompositeFilterInputs {
                input1,
                input2,
                target,
                operator,
                ..
            } => {
                c.filter_kind = encode_composite_operator(operator);
                c.matrix_bias = composite_arithmetic(operator);
                inputs::encode(
                    e.batch,
                    inputs::InputFilter::Composite {
                        source: image(input1)?,
                        backdrop: image(input2)?,
                    },
                    c,
                    tiles,
                    image(target)?,
                )
            }
            DisplacementMapFilterInputs {
                input1,
                input2,
                target,
                displacement,
                ..
            } => {
                configure_displacement(&mut c, displacement);
                displacement::encode(
                    e.batch,
                    c,
                    tiles,
                    image(input1)?,
                    image(input2)?,
                    image(target)?,
                )
            }
            TurbulenceToTarget {
                target,
                turbulence,
                table_index,
                ..
            } => {
                configure_turbulence(&mut c, turbulence, table_index);
                turbulence::encode(
                    e.batch,
                    c,
                    tiles,
                    e.filters
                        .turbulence
                        .ok_or("native turbulence tables missing")?,
                    image(target)?,
                )
            }
            TileFilterInput {
                input,
                target,
                source_region,
                ..
            } => {
                // A disjoint graph input has no repeatable cells. Clear the output
                // instead of passing inverted bounds to raw tile-coordinate math.
                if source_region.is_empty() {
                    c.clear_color = 0;
                    return filter::encode(
                        e.batch,
                        BasicFilter::Clear,
                        c,
                        tiles,
                        None,
                        image(target)?,
                    );
                }
                set_rect(&mut c, source_region);
                filter::encode(
                    e.batch,
                    BasicFilter::Tile,
                    c,
                    tiles,
                    Some(image(input)?),
                    image(target)?,
                )
            }
            ApplyColorMatrixToTarget { target, matrix, .. } => {
                configure_color_matrix(&mut c, matrix);
                filter::encode(
                    e.batch,
                    BasicFilter::ColorMatrix,
                    c,
                    tiles,
                    None,
                    image(target)?,
                )
            }
            ApplyComponentTransferToTarget {
                target,
                table_index,
                ..
            } => {
                c.table_index = table_index;
                transfer::encode(
                    e.batch,
                    c,
                    tiles,
                    e.filters
                        .transfers
                        .ok_or("native transfer tables missing")?,
                    image(target)?,
                )
            }
            ConvolveMatrixToTarget {
                source,
                target,
                matrix,
                kernel_offset,
                ..
            } => {
                configure_convolve(&mut c, matrix, kernel_offset);
                convolve::encode(
                    e.batch,
                    c,
                    tiles,
                    e.filters
                        .convolves
                        .ok_or("native convolution kernels missing")?,
                    image(source)?,
                    image(target)?,
                )
            }
            DiffuseLightingToTarget {
                source,
                target,
                lighting,
                ..
            } => {
                configure_lighting(
                    &mut c,
                    0,
                    lighting.surface_scale,
                    lighting.diffuse_constant,
                    1.0,
                    lighting.lighting_color,
                    lighting.light_source,
                    e.origin,
                );
                lighting::encode(e.batch, c, tiles, image(source)?, image(target)?)
            }
            SpecularLightingToTarget {
                source,
                target,
                lighting,
                ..
            } => {
                configure_lighting(
                    &mut c,
                    1,
                    lighting.surface_scale,
                    lighting.specular_constant,
                    lighting.specular_exponent,
                    lighting.lighting_color,
                    lighting.light_source,
                    e.origin,
                );
                lighting::encode(e.batch, c, tiles, image(source)?, image(target)?)
            }
            LiquidGlassToTarget {
                source,
                blurred,
                target,
                glass,
                region,
                ..
            }
            | RectLiquidGlassCompositeRegion {
                source,
                blurred,
                target,
                glass,
                region,
                ..
            } => {
                configure_rect_liquid_glass(&mut c, glass, region);
                let mode = if let RectLiquidGlassCompositeRegion {
                    blurred_bounds,
                    sampling,
                    ..
                } = kernel
                {
                    set_source(&mut c, blurred_bounds);
                    c.downsample = sampling.factor();
                    c.upsample_filter = encode_blur_upsample_filter(sampling.upsample_filter);
                    glass::Glass::RectangleComposite
                } else {
                    glass::Glass::Effect
                };
                glass::encode(
                    e.batch,
                    mode,
                    c,
                    tiles,
                    image(source)?,
                    image(blurred)?,
                    image(target)?,
                )
            }
            BlurRegionToTarget {
                source,
                target,
                std_dev,
                axis,
                ..
            }
            | BlurRegionPartialToTarget {
                source,
                target,
                std_dev,
                axis,
                ..
            } => {
                c.amount = std_dev;
                c.blur_axis = axis;
                if let BlurRegionPartialToTarget { sample_bounds, .. } = kernel {
                    set_source(&mut c, sample_bounds);
                }
                let radius = (std_dev.max(0.0) * 3.0).ceil().max(1.0);
                let mode = if std_dev > 0.0 && radius <= SHARED_BLUR_MAX_RADIUS as f32 {
                    blur::Blur::Shared
                } else {
                    blur::Blur::Global
                };
                blur::encode(e.batch, mode, c, tiles, image(source)?, image(target)?)
            }
            MorphologyAxisToTarget {
                source,
                target,
                radius,
                operator,
                axis,
                ..
            } => {
                c.morphology_radius = radius;
                c.morphology_operator = match operator {
                    model::MorphologyOperator::Erode => 0,
                    model::MorphologyOperator::Dilate => 1,
                };
                // Morphology has its own uniform field; writing blur_axis silently
                // repeated the horizontal pass instead of eroding/dilating in Y.
                c.morphology_axis = axis;
                morphology::encode(e.batch, c, tiles, image(source)?, image(target)?)
            }
            ApplyColorFilterToTarget {
                target,
                kind,
                amount,
                ..
            } => {
                c.amount = amount;
                c.filter_kind = match kind {
                    ColorFilterKind::Brightness => FILTER_BRIGHTNESS,
                    ColorFilterKind::Contrast => FILTER_CONTRAST,
                    ColorFilterKind::Grayscale => FILTER_GRAYSCALE,
                    ColorFilterKind::HueRotate => FILTER_HUE_ROTATE,
                    ColorFilterKind::Invert => FILTER_INVERT,
                    ColorFilterKind::Opacity => FILTER_OPACITY,
                    ColorFilterKind::Saturate => FILTER_SATURATE,
                    ColorFilterKind::Sepia => FILTER_SEPIA,
                };
                filter::encode(e.batch, BasicFilter::Color, c, tiles, None, image(target)?)
            }
            DownsampleRegion {
                source,
                target,
                source_bounds,
                sampling,
                ..
            }
            | UpsampleRegion {
                source,
                target,
                low_bounds: source_bounds,
                sampling,
                ..
            } => {
                set_rect(&mut c, source_bounds);
                c.downsample = sampling.factor();
                c.downsample_filter = encode_blur_downsample_filter(sampling.downsample_filter);
                c.upsample_filter = encode_blur_upsample_filter(sampling.upsample_filter);
                let mode = if matches!(kernel, DownsampleRegion { .. }) {
                    resample::Resample::Downsample
                } else {
                    resample::Resample::Upsample
                };
                resample::encode(e.batch, mode, c, tiles, image(source)?, image(target)?)
            }
            UpsampleRectCompositeRegion {
                source,
                target,
                low_bounds,
                sampling,
                region,
                ..
            } => {
                configure_region(&mut c, region)?;
                set_source(&mut c, low_bounds);
                c.downsample = sampling.factor();
                c.upsample_filter = encode_blur_upsample_filter(sampling.upsample_filter);
                rectangle::encode(
                    e.batch,
                    rectangle::RectanglePass::UpsampleRectangle {
                        source: image(source)?,
                    },
                    c,
                    tiles,
                    image(target)?,
                )
            }
        }
    }
}
pub(super) fn configure_region(
    c: &mut FilterConfig,
    region: &crate::shared::layer::region::Region,
) -> Result<()> {
    let crate::shared::layer::region::Region::Rect { rect, radius } = region else {
        return Err("rectangle filter requires a rectangle region".into());
    };
    c.rect_x0 = rect.x0 as f32;
    c.rect_y0 = rect.y0 as f32;
    c.rect_x1 = rect.x1 as f32;
    c.rect_y1 = rect.y1 as f32;
    c.radius_top_left = radius.top_left;
    c.radius_top_right = radius.top_right;
    c.radius_bottom_left = radius.bottom_left;
    c.radius_bottom_right = radius.bottom_right;
    Ok(())
}
fn set_rect(c: &mut FilterConfig, b: Bounds) {
    c.rect_x0 = b.x0 as f32;
    c.rect_y0 = b.y0 as f32;
    c.rect_x1 = b.x1 as f32;
    c.rect_y1 = b.y1 as f32;
}
fn set_source(c: &mut FilterConfig, b: Bounds) {
    c.source_x0 = b.x0.max(0) as u32;
    c.source_y0 = b.y0.max(0) as u32;
    c.source_x1 = b.x1.max(0) as u32;
    c.source_y1 = b.y1.max(0) as u32;
}
