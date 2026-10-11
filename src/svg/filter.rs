//! Lower SVG filter primitives and track their canvas-space source domains.
use self::{
    color::{color_matrix_to_filter, component_transfer_to_filter, nearly_eq},
    primitives::{
        composite_operator, convolve_matrix_to_filter, diffuse_lighting_to_filter,
        displacement_map_to_filter, morphology_operator, specular_lighting_to_filter,
        turbulence_to_filter,
    },
};
use super::{
    SvgBuilder, SvgError,
    geometry::{
        blend_mode_to_mix, filter_axis_scale, nonzero_rect_to_kurbo, transform_filter_radii,
        transform_filter_radius_x, transform_filter_radius_y, transform_filter_vector,
        transform_rect_to_bounds,
    },
    paint::color_opacity_to_brush,
};
use crate::{
    Canvas, Filter, Radius, Region,
    shared::{
        bounds::Bounds,
        layer::filter::{FilterInput, FilterPrimitive, FilterPrimitiveKind},
    },
};
use peniko::kurbo::Affine;
use std::collections::HashMap;

mod color;
mod primitives;

pub(super) struct SvgFilterLayer {
    pub(super) filter: Filter,
    pub(super) region: Region,
}

pub(super) fn svg_filter_layers(
    builder: &SvgBuilder,
    canvas: &mut Canvas,
    filters: &[std::sync::Arc<usvg::filter::Filter>],
    region_transform: Affine,
    content_transform: Affine,
    width: u32,
    height: u32,
) -> Result<Vec<SvgFilterLayer>, SvgError> {
    filters
        .iter()
        .filter_map(|filter| {
            svg_filter_layer(
                builder,
                canvas,
                filter.as_ref(),
                region_transform,
                content_transform,
                width,
                height,
            )
            .transpose()
        })
        .collect()
}

fn svg_filter_layer(
    builder: &SvgBuilder,
    canvas: &mut Canvas,
    filter: &usvg::filter::Filter,
    region_transform: Affine,
    content_transform: Affine,
    width: u32,
    height: u32,
) -> Result<Option<SvgFilterLayer>, SvgError> {
    let filter_rect = nonzero_rect_to_kurbo(filter.rect());
    let filter_bounds = transform_rect_to_bounds(filter_rect, region_transform)
        .intersect(Bounds::canvas(width, height));
    let mut graph = SvgFilterGraphBuilder::new(
        builder,
        canvas,
        region_transform,
        content_transform,
        filter_bounds,
    );
    for primitive in filter.primitives() {
        graph.push_primitive(primitive)?;
    }
    let primitives = graph.into_primitives();

    if primitives.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SvgFilterLayer {
            filter: Filter::Graph {
                primitives,
                fixed_region: true,
            },
            region: Region::rect(
                region_transform.transform_rect_bbox(filter_rect),
                Radius::ZERO,
            ),
        }))
    }
}

struct SvgFilterGraphBuilder<'a> {
    builder: &'a SvgBuilder,
    canvas: &'a mut Canvas,
    region_transform: Affine,
    content_transform: Affine,
    value_transform: Affine,
    filter_bounds: Bounds,
    primitives: Vec<FilterPrimitive>,
    source_regions: Vec<Bounds>,
    results: HashMap<String, usize>,
}

struct LoweredSvgFilterPrimitive {
    primitive: FilterPrimitive,
    source_region: Bounds,
}

impl<'a> SvgFilterGraphBuilder<'a> {
    fn new(
        builder: &'a SvgBuilder,
        canvas: &'a mut Canvas,
        region_transform: Affine,
        content_transform: Affine,
        filter_bounds: Bounds,
    ) -> Self {
        Self {
            builder,
            canvas,
            region_transform,
            content_transform,
            value_transform: filter_axis_scale(region_transform),
            filter_bounds,
            primitives: Vec::new(),
            source_regions: Vec::new(),
            results: HashMap::new(),
        }
    }

    fn push_primitive(&mut self, primitive: &usvg::filter::Primitive) -> Result<(), SvgError> {
        let index = self.primitives.len();
        let lowered = self.lower_primitive(primitive)?;
        self.primitives.push(lowered.primitive);
        self.source_regions.push(lowered.source_region);
        self.results.insert(primitive.result().to_string(), index);
        Ok(())
    }

    fn into_primitives(self) -> Vec<FilterPrimitive> {
        self.primitives
    }

    fn lower_primitive(
        &mut self,
        primitive: &usvg::filter::Primitive,
    ) -> Result<LoweredSvgFilterPrimitive, SvgError> {
        let primitive_rect = nonzero_rect_to_kurbo(primitive.rect());
        let region = transform_rect_to_bounds(primitive_rect, self.region_transform);
        let (input, input2, kind) = match primitive.kind() {
            usvg::filter::Kind::GaussianBlur(blur) => {
                let (std_dev_x, std_dev_y) = transform_filter_radii(
                    self.value_transform,
                    blur.std_dev_x().get(),
                    blur.std_dev_y().get(),
                );
                let filter = Filter::Blur {
                    std_dev_x,
                    std_dev_y,
                    sampling: Default::default(),
                };
                (
                    self.input(blur.input(), "feGaussianBlur")?,
                    None,
                    FilterPrimitiveKind::Filter(Box::new(filter)),
                )
            }
            usvg::filter::Kind::DropShadow(shadow) => {
                let (offset_x, offset_y) =
                    transform_filter_vector(self.value_transform, shadow.dx(), shadow.dy());
                let (std_dev_x, std_dev_y) = transform_filter_radii(
                    self.value_transform,
                    shadow.std_dev_x().get(),
                    shadow.std_dev_y().get(),
                );
                let filter = Filter::DropShadow {
                    offset_x,
                    offset_y,
                    std_dev: equal_std_dev(std_dev_x, std_dev_y, "anisotropic feDropShadow")?,
                    brush: color_opacity_to_brush(shadow.color(), shadow.opacity().get()),
                };
                (
                    self.input(shadow.input(), "feDropShadow")?,
                    None,
                    FilterPrimitiveKind::Filter(Box::new(filter)),
                )
            }
            usvg::filter::Kind::ColorMatrix(matrix) => {
                let kind = filter_to_primitive_kind(color_matrix_to_filter(matrix.kind()));
                (self.input(matrix.input(), "feColorMatrix")?, None, kind)
            }
            usvg::filter::Kind::ComponentTransfer(transfer) => {
                let kind = filter_to_primitive_kind(component_transfer_to_filter(transfer));
                (
                    self.input(transfer.input(), "feComponentTransfer")?,
                    None,
                    kind,
                )
            }
            usvg::filter::Kind::Blend(blend) => (
                self.input(blend.input1(), "feBlend")?,
                Some(self.input(blend.input2(), "feBlend")?),
                FilterPrimitiveKind::Blend {
                    mode: blend_mode_to_mix(blend.mode()),
                },
            ),
            usvg::filter::Kind::Composite(composite) => (
                self.input(composite.input1(), "feComposite")?,
                Some(self.input(composite.input2(), "feComposite")?),
                FilterPrimitiveKind::Composite {
                    operator: composite_operator(composite.operator()),
                },
            ),
            usvg::filter::Kind::ConvolveMatrix(convolve) => (
                self.input(convolve.input(), "feConvolveMatrix")?,
                None,
                FilterPrimitiveKind::Filter(Box::new(convolve_matrix_to_filter(convolve))),
            ),
            usvg::filter::Kind::DiffuseLighting(lighting) => (
                self.input(lighting.input(), "feDiffuseLighting")?,
                None,
                FilterPrimitiveKind::Filter(Box::new(diffuse_lighting_to_filter(
                    lighting,
                    self.region_transform,
                ))),
            ),
            usvg::filter::Kind::DisplacementMap(displacement) => (
                self.input(displacement.input1(), "feDisplacementMap")?,
                Some(self.input(displacement.input2(), "feDisplacementMap")?),
                FilterPrimitiveKind::DisplacementMap(displacement_map_to_filter(
                    displacement,
                    primitive.color_interpolation(),
                    self.value_transform,
                )),
            ),
            usvg::filter::Kind::Flood(flood) => (
                FilterInput::SourceGraphic,
                None,
                FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                    brush: color_opacity_to_brush(flood.color(), flood.opacity().get()),
                })),
            ),
            usvg::filter::Kind::Image(image) => (
                FilterInput::SourceGraphic,
                None,
                FilterPrimitiveKind::Image {
                    brush: self.builder.filter_image_to_brush(
                        self.canvas,
                        image,
                        self.filter_bounds,
                        primitive_rect,
                        self.region_transform,
                        self.content_transform,
                    )?,
                },
            ),
            usvg::filter::Kind::Merge(merge) => (
                FilterInput::SourceGraphic,
                None,
                FilterPrimitiveKind::Merge {
                    inputs: self.inputs(merge.inputs(), "feMerge")?,
                },
            ),
            usvg::filter::Kind::Morphology(morphology) => (
                self.input(morphology.input(), "feMorphology")?,
                None,
                FilterPrimitiveKind::Filter(Box::new(Filter::Morphology {
                    radius_x: transform_filter_radius_x(
                        self.value_transform,
                        morphology.radius_x().get(),
                    ),
                    radius_y: transform_filter_radius_y(
                        self.value_transform,
                        morphology.radius_y().get(),
                    ),
                    operator: morphology_operator(morphology.operator()),
                })),
            ),
            usvg::filter::Kind::Offset(offset) => {
                let (dx, dy) =
                    transform_filter_vector(self.value_transform, offset.dx(), offset.dy());
                (
                    self.input(offset.input(), "feOffset")?,
                    None,
                    FilterPrimitiveKind::Filter(Box::new(Filter::Offset { dx, dy })),
                )
            }
            usvg::filter::Kind::SpecularLighting(lighting) => (
                self.input(lighting.input(), "feSpecularLighting")?,
                None,
                FilterPrimitiveKind::Filter(Box::new(specular_lighting_to_filter(
                    lighting,
                    self.region_transform,
                ))),
            ),
            usvg::filter::Kind::Tile(tile) => {
                let input = self.input(tile.input(), "feTile")?;
                (
                    input,
                    None,
                    FilterPrimitiveKind::Tile {
                        source_region: self.input_source_region(input),
                    },
                )
            }
            usvg::filter::Kind::Turbulence(turbulence) => (
                FilterInput::SourceGraphic,
                None,
                FilterPrimitiveKind::Turbulence(turbulence_to_filter(
                    turbulence,
                    primitive.color_interpolation(),
                    self.region_transform,
                    self.filter_bounds,
                )),
            ),
        };
        let source_region = self.output_source_region(input, input2, &kind, region);
        Ok(LoweredSvgFilterPrimitive {
            primitive: FilterPrimitive {
                input,
                input2,
                region,
                linear_rgb: primitive.color_interpolation()
                    == usvg::filter::ColorInterpolation::LinearRGB,
                kind,
            },
            source_region,
        })
    }

    fn output_source_region(
        &self,
        input: FilterInput,
        input2: Option<FilterInput>,
        kind: &FilterPrimitiveKind,
        region: Bounds,
    ) -> Bounds {
        let region = region.intersect(self.filter_bounds);
        // Use the lowered parameters: their transforms and resolved graph edges
        // already define the exact values consumed by the renderer.
        let source_region = match kind {
            FilterPrimitiveKind::Image { .. } | FilterPrimitiveKind::Turbulence(_) => region,
            FilterPrimitiveKind::Filter(filter) => match filter.as_ref() {
                Filter::Flood { .. } => region,
                Filter::Blur {
                    std_dev_x,
                    std_dev_y,
                    ..
                } => self
                    .input_source_region(input)
                    .outset(blur_outset(std_dev_x.max(*std_dev_y))),
                Filter::Morphology {
                    radius_x,
                    radius_y,
                    operator,
                } => {
                    let radius = match operator {
                        crate::MorphologyOperator::Dilate => {
                            radius_x.max(*radius_y).max(0.0).ceil() as i32
                        }
                        crate::MorphologyOperator::Erode => 0,
                    };
                    self.input_source_region(input).outset(radius)
                }
                _ => self.input_source_region(input),
            },
            FilterPrimitiveKind::Blend { .. } => {
                self.input_source_region(input)
                    .union(self.input_source_region(
                        input2.expect("feBlend lowering produced no second input"),
                    ))
            }
            FilterPrimitiveKind::Composite { operator } => {
                let first = self.input_source_region(input);
                let second = self.input_source_region(
                    input2.expect("feComposite lowering produced no second input"),
                );
                match operator {
                    crate::CompositeOperator::In => first.intersect(second),
                    crate::CompositeOperator::Out => first,
                    _ => first.union(second),
                }
            }
            FilterPrimitiveKind::DisplacementMap(displacement) => {
                self.input_source_region(input).outset(
                    (displacement.scale_x.abs().max(displacement.scale_y.abs()) * 0.5).ceil()
                        as i32,
                )
            }
            FilterPrimitiveKind::Merge { inputs } => inputs
                .iter()
                .map(|input| self.input_source_region(*input))
                .fold(Bounds::new(0, 0, 0, 0), Bounds::union),
            FilterPrimitiveKind::Tile { source_region } => {
                if source_region.is_empty() {
                    return Bounds::new(0, 0, 0, 0);
                }
                region
            }
            FilterPrimitiveKind::Identity => self.input_source_region(input),
            FilterPrimitiveKind::ShaderPipeline { .. } => {
                unreachable!("SVG lowering only produces built-in primitives")
            }
        };
        source_region.intersect(region)
    }

    fn input_source_region(&self, input: FilterInput) -> Bounds {
        match input {
            FilterInput::SourceGraphic | FilterInput::SourceAlpha => self.filter_bounds,
            FilterInput::Primitive(index) => self.source_regions[index],
        }
    }

    fn input(&self, input: &usvg::filter::Input, primitive: &str) -> Result<FilterInput, SvgError> {
        match input {
            usvg::filter::Input::SourceGraphic => Ok(FilterInput::SourceGraphic),
            usvg::filter::Input::SourceAlpha => Ok(FilterInput::SourceAlpha),
            usvg::filter::Input::Reference(reference) => self
                .results
                .get(reference)
                .copied()
                .map(FilterInput::Primitive)
                .ok_or_else(|| SvgError::unsupported(format!("{primitive} input graph"))),
        }
    }

    fn inputs(
        &self,
        inputs: &[usvg::filter::Input],
        primitive: &str,
    ) -> Result<Vec<FilterInput>, SvgError> {
        inputs
            .iter()
            .map(|input| self.input(input, primitive))
            .collect()
    }
}

fn filter_to_primitive_kind(filter: Option<Filter>) -> FilterPrimitiveKind {
    match filter {
        Some(filter) => FilterPrimitiveKind::Filter(Box::new(filter)),
        None => FilterPrimitiveKind::Identity,
    }
}

fn blur_outset(std_dev: f32) -> i32 {
    (std_dev.max(0.0) * 3.0).ceil() as i32
}

fn equal_std_dev(x: f32, y: f32, feature: &str) -> Result<f32, SvgError> {
    if nearly_eq(x, y) {
        Ok(x)
    } else {
        Err(SvgError::unsupported(feature))
    }
}

#[cfg(test)]
mod tests;
