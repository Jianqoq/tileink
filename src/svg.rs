use std::{collections::HashMap, error::Error, fmt, sync::Arc};

use peniko::{
    Color, ColorStop, Compose, Extend, Gradient, Mix,
    kurbo::{Affine, BezPath, Cap, Join, Rect, Shape, Stroke},
};
use usvg::{Node, Paint, PaintOrder, SpreadMethod, tiny_skia_path::PathSegment};

use crate::{
    Brush, CpuRenderer, FillRule, Filter, Radius, Region, Scene,
    shared::{
        bounds::Bounds,
        brush::PatternBrush,
        layer::filter::{
            COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE, ComponentTransferTable,
            CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting, FilterInput,
            FilterPrimitive, FilterPrimitiveKind, LightSource, MorphologyOperator,
            SpecularLighting,
        },
    },
};

#[derive(Clone, Copy, Debug)]
pub struct SvgOptions {
    pub tolerance: f64,
}

const MAX_PATTERN_DEPTH: u8 = 16;

impl Default for SvgOptions {
    fn default() -> Self {
        Self { tolerance: 0.1 }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SvgError {
    feature: String,
}

impl SvgError {
    pub fn unsupported(feature: impl Into<String>) -> Self {
        Self {
            feature: feature.into(),
        }
    }

    pub fn feature(&self) -> &str {
        &self.feature
    }
}

impl fmt::Display for SvgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported SVG feature: {}", self.feature)
    }
}

impl Error for SvgError {}

impl Scene {
    /// Appends a parsed `usvg` tree by lowering the reduced SVG tree into Scene primitives.
    ///
    /// The conversion is transactional: unsupported SVG features return an error before this
    /// scene is modified. This prevents partial SVG output from looking like a successful render.
    pub fn push_svg(&mut self, tree: &usvg::Tree) -> Result<(), SvgError> {
        self.push_svg_with_options(tree, SvgOptions::default())
    }

    pub fn push_svg_with_options(
        &mut self,
        tree: &usvg::Tree,
        options: SvgOptions,
    ) -> Result<(), SvgError> {
        let mut svg_scene = Scene::new(self.width, self.height);
        SvgBuilder::new(options).push_tree(&mut svg_scene, tree)?;
        self.merge(svg_scene);
        Ok(())
    }
}

struct SvgBuilder {
    options: SvgOptions,
    base_transform: Affine,
    pattern_depth: u8,
}

impl SvgBuilder {
    fn new(options: SvgOptions) -> Self {
        Self {
            options,
            base_transform: Affine::IDENTITY,
            pattern_depth: 0,
        }
    }

    fn push_tree(&self, scene: &mut Scene, tree: &usvg::Tree) -> Result<(), SvgError> {
        self.push_group(scene, tree.root())
    }

    fn push_group(&self, scene: &mut Scene, group: &usvg::Group) -> Result<(), SvgError> {
        if group.mask().is_some() {
            return Err(SvgError::unsupported("mask"));
        }
        if group.isolate()
            && group.opacity().get() >= 1.0
            && group.blend_mode() == usvg::BlendMode::Normal
            && group.clip_path().is_none()
        {
            return Err(SvgError::unsupported(
                "isolated group without opacity or blend",
            ));
        }
        let filter_layers = svg_filter_layers(group.filters())?;

        let mut pushed_layers = 0;
        if let Some(clip) = group.clip_path() {
            pushed_layers += self.push_clip_path_layers(scene, clip)?;
        }

        let layer_path = || {
            self.base_transform * rect_path(nonzero_rect_to_kurbo(group.abs_layer_bounding_box()))
        };
        if group.blend_mode() != usvg::BlendMode::Normal {
            scene.push_blend_layer(
                layer_path(),
                Affine::IDENTITY,
                self.options.tolerance,
                blend_mode_to_mix(group.blend_mode()),
                Compose::SrcOver,
            );
            pushed_layers += 1;
        }
        if group.opacity().get() < 1.0 {
            scene.push_opacity_layer(
                layer_path(),
                Affine::IDENTITY,
                self.options.tolerance,
                group.opacity().get(),
            );
            pushed_layers += 1;
        }
        for layer in filter_layers.into_iter().rev() {
            scene.push_filter_layer(
                layer.filter,
                transform_region(layer.region, self.base_transform),
            );
            pushed_layers += 1;
        }

        for child in group.children() {
            self.push_node(scene, child)?;
        }

        for _ in 0..pushed_layers {
            scene.pop_layer();
        }
        Ok(())
    }

    fn push_node(&self, scene: &mut Scene, node: &Node) -> Result<(), SvgError> {
        match node {
            Node::Group(group) => self.push_group(scene, group),
            Node::Path(path) => self.push_path(scene, path),
            Node::Text(text) => self.push_group(scene, text.flattened()),
            Node::Image(_) => Err(SvgError::unsupported("image")),
        }
    }

    fn push_path(&self, scene: &mut Scene, path: &usvg::Path) -> Result<(), SvgError> {
        if !path.is_visible() {
            return Ok(());
        }

        let data = tiny_path_to_bez(path.data());
        let transform = self.base_transform * transform_to_affine(path.abs_transform());
        match path.paint_order() {
            PaintOrder::FillAndStroke => {
                self.push_fill(scene, path, &data, transform)?;
                self.push_stroke(scene, path, &data, transform)?;
            }
            PaintOrder::StrokeAndFill => {
                self.push_stroke(scene, path, &data, transform)?;
                self.push_fill(scene, path, &data, transform)?;
            }
        }
        Ok(())
    }

    fn push_fill(
        &self,
        scene: &mut Scene,
        path: &usvg::Path,
        data: &BezPath,
        transform: Affine,
    ) -> Result<(), SvgError> {
        let Some(fill) = path.fill() else {
            return Ok(());
        };
        if fill.opacity().get() <= 0.0 {
            return Ok(());
        }

        let brush = self.paint_to_brush(fill.paint(), fill.opacity().get())?;
        scene.push_path(
            data.clone(),
            brush,
            transform,
            fill_rule(fill.rule()),
            self.options.tolerance,
        );
        Ok(())
    }

    fn push_stroke(
        &self,
        scene: &mut Scene,
        path: &usvg::Path,
        data: &BezPath,
        transform: Affine,
    ) -> Result<(), SvgError> {
        let Some(stroke) = path.stroke() else {
            return Ok(());
        };
        if stroke.opacity().get() <= 0.0 {
            return Ok(());
        }

        let brush = self.paint_to_brush(stroke.paint(), stroke.opacity().get())?;
        let stroke_style = stroke_to_kurbo(stroke)?;
        scene.push_stroke(
            data.clone(),
            stroke_style,
            brush,
            transform,
            FillRule::NonZero,
            self.options.tolerance,
        );
        Ok(())
    }

    fn push_clip_path_layers(
        &self,
        scene: &mut Scene,
        clip: &usvg::ClipPath,
    ) -> Result<usize, SvgError> {
        let mut pushed = 0;
        if let Some(parent) = clip.clip_path() {
            pushed += self.push_clip_path_layers(scene, parent)?;
        }

        let (path, rule) = self.single_clip_path(clip)?;
        scene.push_clip_layer(
            path,
            self.base_transform * transform_to_affine(clip.transform()),
            rule,
            self.options.tolerance,
        );
        Ok(pushed + 1)
    }

    fn single_clip_path(&self, clip: &usvg::ClipPath) -> Result<(BezPath, FillRule), SvgError> {
        let mut paths = Vec::new();
        self.collect_clip_paths(clip.root(), &mut paths)?;
        match paths.len() {
            0 => Err(SvgError::unsupported("empty clipPath")),
            1 => Ok(paths.pop().unwrap()),
            _ => Err(SvgError::unsupported(
                "clipPath with multiple drawable children",
            )),
        }
    }

    fn collect_clip_paths(
        &self,
        group: &usvg::Group,
        paths: &mut Vec<(BezPath, FillRule)>,
    ) -> Result<(), SvgError> {
        if group.mask().is_some() {
            return Err(SvgError::unsupported("mask inside clipPath"));
        }
        if !group.filters().is_empty() {
            return Err(SvgError::unsupported("filter inside clipPath"));
        }
        if group.clip_path().is_some() {
            return Err(SvgError::unsupported("nested clipPath content clip"));
        }

        for child in group.children() {
            match child {
                Node::Group(group) => self.collect_clip_paths(group, paths)?,
                Node::Path(path) => {
                    if path.is_visible() {
                        let rule = path
                            .fill()
                            .map(|fill| fill_rule(fill.rule()))
                            .unwrap_or(FillRule::NonZero);
                        paths.push((
                            transform_to_affine(path.abs_transform())
                                * tiny_path_to_bez(path.data()),
                            rule,
                        ));
                    }
                }
                Node::Text(text) => self.collect_clip_paths(text.flattened(), paths)?,
                Node::Image(_) => return Err(SvgError::unsupported("image inside clipPath")),
            }
        }
        Ok(())
    }

    fn paint_to_brush(&self, paint: &Paint, opacity: f32) -> Result<Brush, SvgError> {
        match paint {
            Paint::Color(color) => Ok(color_opacity_to_brush(*color, opacity)),
            Paint::LinearGradient(source) => {
                let transform = source.transform();
                let stops = gradient_stops(source.stops(), opacity);
                let gradient = Gradient::new_linear(
                    (source.x1() as f64, source.y1() as f64),
                    (source.x2() as f64, source.y2() as f64),
                )
                .with_extend(spread_method(source.spread_method()))
                .with_stops(stops.as_slice());
                let mut brush = Brush::from_gradient(&gradient);
                let Brush::Linear(linear) = &mut brush else {
                    unreachable!();
                };
                linear.transform = inverse_transform_array(transform, "gradientTransform")?;
                Ok(brush)
            }
            Paint::RadialGradient(source) => {
                let transform = source.transform();
                let stops = gradient_stops(source.stops(), opacity);
                let gradient = Gradient::new_two_point_radial(
                    (source.fx() as f64, source.fy() as f64),
                    source.fr().get(),
                    (source.cx() as f64, source.cy() as f64),
                    source.r().get(),
                )
                .with_extend(spread_method(source.spread_method()))
                .with_stops(stops.as_slice());
                let mut brush = Brush::from_gradient(&gradient);
                let Brush::Radial(radial) = &mut brush else {
                    unreachable!();
                };
                radial.transform = inverse_transform_array(transform, "gradientTransform")?;
                Ok(brush)
            }
            Paint::Pattern(pattern) => self.pattern_to_brush(pattern, opacity),
        }
    }

    fn pattern_to_brush(&self, pattern: &usvg::Pattern, opacity: f32) -> Result<Brush, SvgError> {
        if self.pattern_depth >= MAX_PATTERN_DEPTH {
            return Err(SvgError::unsupported("recursive pattern paint"));
        }

        let rect = pattern.rect();
        let width = rect.width();
        let height = rect.height();
        let tile_width = width.ceil().max(1.0) as u32;
        let tile_height = height.ceil().max(1.0) as u32;
        // Render pattern content once in tile pixel space, then reuse the same
        // world-to-tile transform for brush sampling so fractional tile sizes
        // keep the SVG repeat period instead of snapping to integer user units.
        let tile_transform =
            Affine::scale_non_uniform(
                f64::from(tile_width) / f64::from(width),
                f64::from(tile_height) / f64::from(height),
            ) * Affine::translate((-f64::from(rect.left()), -f64::from(rect.top())));

        let mut tile_scene = Scene::new(tile_width, tile_height);
        SvgBuilder {
            options: self.options,
            base_transform: tile_transform,
            pattern_depth: self.pattern_depth + 1,
        }
        .push_group(&mut tile_scene, pattern.root())?;

        let mut renderer = CpuRenderer::new(tile_width, tile_height, Color::TRANSPARENT);
        renderer.render(&tile_scene);

        let Some(pattern_inverse) = pattern.transform().invert() else {
            return Err(SvgError::unsupported("non-invertible patternTransform"));
        };
        Ok(Brush::Pattern(PatternBrush {
            image: Arc::new(renderer.image().clone()),
            transform: affine_to_array(tile_transform * transform_to_affine(pattern_inverse)),
            opacity: opacity_to_u8(opacity),
        }))
    }
}

struct SvgFilterLayer {
    filter: Filter,
    region: Region,
}

fn svg_filter_layers(
    filters: &[std::sync::Arc<usvg::filter::Filter>],
) -> Result<Vec<SvgFilterLayer>, SvgError> {
    filters
        .iter()
        .filter_map(|filter| svg_filter_layer(filter.as_ref()).transpose())
        .collect()
}

fn svg_filter_layer(filter: &usvg::filter::Filter) -> Result<Option<SvgFilterLayer>, SvgError> {
    let mut primitives = Vec::new();
    let mut results = HashMap::new();
    for primitive in filter.primitives() {
        let index = primitives.len();
        primitives.push(svg_filter_primitive(primitive, &results)?);
        results.insert(primitive.result().to_string(), index);
    }

    if primitives.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SvgFilterLayer {
            filter: Filter::Graph {
                primitives,
                fixed_region: true,
            },
            region: Region::rect(nonzero_rect_to_kurbo(filter.rect()), Radius::all(0.0)),
        }))
    }
}

fn svg_filter_primitive(
    primitive: &usvg::filter::Primitive,
    results: &HashMap<String, usize>,
) -> Result<FilterPrimitive, SvgError> {
    let region = nonzero_rect_to_bounds(primitive.rect());
    let (input, input2, kind) = match primitive.kind() {
        usvg::filter::Kind::GaussianBlur(blur) => {
            let filter = Filter::Blur(equal_std_dev(
                blur.std_dev_x().get(),
                blur.std_dev_y().get(),
                "anisotropic feGaussianBlur",
            )?);
            (
                svg_filter_input(blur.input(), results, "feGaussianBlur")?,
                None,
                FilterPrimitiveKind::Filter(Box::new(filter)),
            )
        }
        usvg::filter::Kind::DropShadow(shadow) => {
            let filter = Filter::DropShadow {
                offset_x: shadow.dx(),
                offset_y: shadow.dy(),
                radius: equal_std_dev(
                    shadow.std_dev_x().get(),
                    shadow.std_dev_y().get(),
                    "anisotropic feDropShadow",
                )?,
                brush: color_opacity_to_brush(shadow.color(), shadow.opacity().get()),
            };
            (
                svg_filter_input(shadow.input(), results, "feDropShadow")?,
                None,
                FilterPrimitiveKind::Filter(Box::new(filter)),
            )
        }
        usvg::filter::Kind::ColorMatrix(matrix) => {
            let kind = filter_to_primitive_kind(color_matrix_to_filter(matrix.kind())?);
            (
                svg_filter_input(matrix.input(), results, "feColorMatrix")?,
                None,
                kind,
            )
        }
        usvg::filter::Kind::ComponentTransfer(transfer) => {
            let kind = filter_to_primitive_kind(component_transfer_to_filter(transfer)?);
            (
                svg_filter_input(transfer.input(), results, "feComponentTransfer")?,
                None,
                kind,
            )
        }
        usvg::filter::Kind::Blend(blend) => (
            svg_filter_input(blend.input1(), results, "feBlend")?,
            Some(svg_filter_input(blend.input2(), results, "feBlend")?),
            FilterPrimitiveKind::Blend {
                mode: blend_mode_to_mix(blend.mode()),
            },
        ),
        usvg::filter::Kind::Composite(composite) => (
            svg_filter_input(composite.input1(), results, "feComposite")?,
            Some(svg_filter_input(
                composite.input2(),
                results,
                "feComposite",
            )?),
            FilterPrimitiveKind::Composite {
                operator: composite_operator(composite.operator()),
            },
        ),
        usvg::filter::Kind::ConvolveMatrix(convolve) => (
            svg_filter_input(convolve.input(), results, "feConvolveMatrix")?,
            None,
            FilterPrimitiveKind::Filter(Box::new(convolve_matrix_to_filter(convolve))),
        ),
        usvg::filter::Kind::DiffuseLighting(lighting) => (
            svg_filter_input(lighting.input(), results, "feDiffuseLighting")?,
            None,
            FilterPrimitiveKind::Filter(Box::new(diffuse_lighting_to_filter(lighting))),
        ),
        usvg::filter::Kind::DisplacementMap(_) => {
            return Err(SvgError::unsupported("feDisplacementMap"));
        }
        usvg::filter::Kind::Flood(flood) => (
            FilterInput::SourceGraphic,
            None,
            FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                brush: color_opacity_to_brush(flood.color(), flood.opacity().get()),
            })),
        ),
        usvg::filter::Kind::Image(_) => return Err(SvgError::unsupported("feImage")),
        usvg::filter::Kind::Merge(merge) => (
            FilterInput::SourceGraphic,
            None,
            FilterPrimitiveKind::Merge {
                inputs: svg_filter_inputs(merge.inputs(), results, "feMerge")?,
            },
        ),
        usvg::filter::Kind::Morphology(morphology) => (
            svg_filter_input(morphology.input(), results, "feMorphology")?,
            None,
            FilterPrimitiveKind::Filter(Box::new(Filter::Morphology {
                radius_x: morphology.radius_x().get(),
                radius_y: morphology.radius_y().get(),
                operator: morphology_operator(morphology.operator()),
            })),
        ),
        usvg::filter::Kind::Offset(offset) => (
            svg_filter_input(offset.input(), results, "feOffset")?,
            None,
            FilterPrimitiveKind::Filter(Box::new(Filter::Offset {
                dx: offset.dx(),
                dy: offset.dy(),
            })),
        ),
        usvg::filter::Kind::SpecularLighting(lighting) => (
            svg_filter_input(lighting.input(), results, "feSpecularLighting")?,
            None,
            FilterPrimitiveKind::Filter(Box::new(specular_lighting_to_filter(lighting))),
        ),
        usvg::filter::Kind::Tile(_) => return Err(SvgError::unsupported("feTile")),
        usvg::filter::Kind::Turbulence(_) => return Err(SvgError::unsupported("feTurbulence")),
    };
    Ok(FilterPrimitive {
        input,
        input2,
        region,
        kind,
    })
}

fn filter_to_primitive_kind(filter: Option<Filter>) -> FilterPrimitiveKind {
    match filter {
        Some(filter) => FilterPrimitiveKind::Filter(Box::new(filter)),
        None => FilterPrimitiveKind::Identity,
    }
}

fn svg_filter_input(
    input: &usvg::filter::Input,
    results: &HashMap<String, usize>,
    primitive: &str,
) -> Result<FilterInput, SvgError> {
    match input {
        usvg::filter::Input::SourceGraphic => Ok(FilterInput::SourceGraphic),
        usvg::filter::Input::SourceAlpha => Ok(FilterInput::SourceAlpha),
        usvg::filter::Input::Reference(reference) => results
            .get(reference)
            .copied()
            .map(FilterInput::Primitive)
            .ok_or_else(|| SvgError::unsupported(format!("{primitive} input graph"))),
    }
}

fn svg_filter_inputs(
    inputs: &[usvg::filter::Input],
    results: &HashMap<String, usize>,
    primitive: &str,
) -> Result<Vec<FilterInput>, SvgError> {
    inputs
        .iter()
        .map(|input| svg_filter_input(input, results, primitive))
        .collect()
}

fn composite_operator(operator: usvg::filter::CompositeOperator) -> CompositeOperator {
    match operator {
        usvg::filter::CompositeOperator::Over => CompositeOperator::Over,
        usvg::filter::CompositeOperator::In => CompositeOperator::In,
        usvg::filter::CompositeOperator::Out => CompositeOperator::Out,
        usvg::filter::CompositeOperator::Atop => CompositeOperator::Atop,
        usvg::filter::CompositeOperator::Xor => CompositeOperator::Xor,
        usvg::filter::CompositeOperator::Arithmetic { k1, k2, k3, k4 } => {
            CompositeOperator::Arithmetic { k1, k2, k3, k4 }
        }
    }
}

fn convolve_matrix_to_filter(convolve: &usvg::filter::ConvolveMatrix) -> Filter {
    let matrix = convolve.matrix();
    Filter::ConvolveMatrix(ConvolveMatrix {
        columns: matrix.columns(),
        rows: matrix.rows(),
        target_x: matrix.target_x(),
        target_y: matrix.target_y(),
        data: matrix.data().to_vec(),
        divisor: convolve.divisor().get(),
        bias: convolve.bias(),
        edge_mode: convolve_edge_mode(convolve.edge_mode()),
        preserve_alpha: convolve.preserve_alpha(),
    })
}

fn convolve_edge_mode(edge_mode: usvg::filter::EdgeMode) -> ConvolveEdgeMode {
    match edge_mode {
        usvg::filter::EdgeMode::None => ConvolveEdgeMode::None,
        usvg::filter::EdgeMode::Duplicate => ConvolveEdgeMode::Duplicate,
        usvg::filter::EdgeMode::Wrap => ConvolveEdgeMode::Wrap,
    }
}

fn diffuse_lighting_to_filter(lighting: &usvg::filter::DiffuseLighting) -> Filter {
    Filter::DiffuseLighting(DiffuseLighting {
        surface_scale: lighting.surface_scale(),
        diffuse_constant: lighting.diffuse_constant(),
        lighting_color: color_to_rgb(lighting.lighting_color()),
        light_source: light_source(lighting.light_source()),
    })
}

fn specular_lighting_to_filter(lighting: &usvg::filter::SpecularLighting) -> Filter {
    Filter::SpecularLighting(SpecularLighting {
        surface_scale: lighting.surface_scale(),
        specular_constant: lighting.specular_constant(),
        specular_exponent: lighting.specular_exponent(),
        lighting_color: color_to_rgb(lighting.lighting_color()),
        light_source: light_source(lighting.light_source()),
    })
}

fn light_source(source: usvg::filter::LightSource) -> LightSource {
    match source {
        usvg::filter::LightSource::DistantLight(light) => LightSource::Distant {
            azimuth: light.azimuth,
            elevation: light.elevation,
        },
        usvg::filter::LightSource::PointLight(light) => LightSource::Point {
            x: light.x,
            y: light.y,
            z: light.z,
        },
        usvg::filter::LightSource::SpotLight(light) => LightSource::Spot {
            x: light.x,
            y: light.y,
            z: light.z,
            points_at_x: light.points_at_x,
            points_at_y: light.points_at_y,
            points_at_z: light.points_at_z,
            specular_exponent: light.specular_exponent.get(),
            limiting_cone_angle: light.limiting_cone_angle,
        },
    }
}

fn morphology_operator(operator: usvg::filter::MorphologyOperator) -> MorphologyOperator {
    match operator {
        usvg::filter::MorphologyOperator::Erode => MorphologyOperator::Erode,
        usvg::filter::MorphologyOperator::Dilate => MorphologyOperator::Dilate,
    }
}

fn color_matrix_to_filter(
    matrix: &usvg::filter::ColorMatrixKind,
) -> Result<Option<Filter>, SvgError> {
    match matrix {
        usvg::filter::ColorMatrixKind::Saturate(amount) => Ok(Some(Filter::Saturate(amount.get()))),
        usvg::filter::ColorMatrixKind::HueRotate(angle) => Ok(Some(Filter::HueRotate(*angle))),
        usvg::filter::ColorMatrixKind::Matrix(values) => {
            if matrix_is_identity(values) {
                Ok(None)
            } else if let Some(amount) = standard_grayscale_amount(values) {
                Ok(Some(Filter::Grayscale(amount)))
            } else if let Some(amount) = standard_sepia_amount(values) {
                Ok(Some(Filter::Sepia(amount)))
            } else {
                let mut matrix = [0.0; 20];
                matrix.copy_from_slice(values);
                Ok(Some(Filter::ColorMatrix(matrix)))
            }
        }
        usvg::filter::ColorMatrixKind::LuminanceToAlpha => Ok(Some(Filter::ColorMatrix([
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.2126,
            0.7152, 0.0722, 0.0, 0.0,
        ]))),
    }
}

fn component_transfer_to_filter(
    transfer: &usvg::filter::ComponentTransfer,
) -> Result<Option<Filter>, SvgError> {
    let r = transfer.func_r();
    let g = transfer.func_g();
    let b = transfer.func_b();
    let a = transfer.func_a();

    if transfer_is_identity(r)
        && transfer_is_identity(g)
        && transfer_is_identity(b)
        && transfer_is_identity(a)
    {
        return Ok(None);
    }

    let opacity_amount =
        (transfer_is_identity(r) && transfer_is_identity(g) && transfer_is_identity(b))
            .then(|| transfer_opacity_amount(a))
            .flatten();
    if let Some(amount) = opacity_amount {
        return Ok(Some(Filter::Opacity(amount)));
    }

    if transfer_is_identity(a) {
        if let Some((slope, intercept)) = same_linear_rgb(r, g, b) {
            if nearly_eq(intercept, 0.0) {
                return Ok(Some(Filter::Brightness(slope)));
            }
            if nearly_eq(intercept, 0.5 - 0.5 * slope) {
                return Ok(Some(Filter::Contrast(slope)));
            }
        }

        if let Some(amount) = same_invert_table_rgb(r, g, b) {
            return Ok(Some(Filter::Invert(amount)));
        }
    }

    Ok(Some(Filter::ComponentTransfer(component_transfer_table(
        r, g, b, a,
    ))))
}

fn transfer_opacity_amount(transfer: &usvg::filter::TransferFunction) -> Option<f32> {
    if let Some([start, amount]) = transfer_table_pair(transfer) {
        return nearly_eq(start, 0.0).then_some(amount);
    }
    let (slope, intercept) = transfer_linear(transfer)?;
    nearly_eq(intercept, 0.0).then_some(slope)
}

fn same_invert_table_rgb(
    r: &usvg::filter::TransferFunction,
    g: &usvg::filter::TransferFunction,
    b: &usvg::filter::TransferFunction,
) -> Option<f32> {
    let [r0, r1] = transfer_table_pair(r)?;
    let [g0, g1] = transfer_table_pair(g)?;
    let [b0, b1] = transfer_table_pair(b)?;
    (nearly_eq(r0, g0)
        && nearly_eq(r0, b0)
        && nearly_eq(r1, g1)
        && nearly_eq(r1, b1)
        && nearly_eq(r1, 1.0 - r0))
    .then_some(r0)
}

fn same_linear_rgb(
    r: &usvg::filter::TransferFunction,
    g: &usvg::filter::TransferFunction,
    b: &usvg::filter::TransferFunction,
) -> Option<(f32, f32)> {
    let (rs, ri) = transfer_linear(r)?;
    let (gs, gi) = transfer_linear(g)?;
    let (bs, bi) = transfer_linear(b)?;
    if nearly_eq(rs, gs) && nearly_eq(rs, bs) && nearly_eq(ri, gi) && nearly_eq(ri, bi) {
        Some((rs, ri))
    } else {
        None
    }
}

fn transfer_linear(transfer: &usvg::filter::TransferFunction) -> Option<(f32, f32)> {
    match transfer {
        usvg::filter::TransferFunction::Linear { slope, intercept } => Some((*slope, *intercept)),
        _ => None,
    }
}

fn transfer_table_pair(transfer: &usvg::filter::TransferFunction) -> Option<[f32; 2]> {
    match transfer {
        usvg::filter::TransferFunction::Table(values) if values.len() == 2 => {
            Some([values[0], values[1]])
        }
        _ => None,
    }
}

fn transfer_is_identity(transfer: &usvg::filter::TransferFunction) -> bool {
    match transfer {
        usvg::filter::TransferFunction::Identity => true,
        // usvg uses empty vectors for missing or invalid tableValues; treat them as no-op.
        usvg::filter::TransferFunction::Table(values)
        | usvg::filter::TransferFunction::Discrete(values)
            if values.is_empty() =>
        {
            true
        }
        usvg::filter::TransferFunction::Linear { slope, intercept } => {
            nearly_eq(*slope, 1.0) && nearly_eq(*intercept, 0.0)
        }
        usvg::filter::TransferFunction::Table(values) if values.len() == 2 => {
            nearly_eq(values[0], 0.0) && nearly_eq(values[1], 1.0)
        }
        _ => false,
    }
}

fn component_transfer_table(
    r: &usvg::filter::TransferFunction,
    g: &usvg::filter::TransferFunction,
    b: &usvg::filter::TransferFunction,
    a: &usvg::filter::TransferFunction,
) -> Box<ComponentTransferTable> {
    let mut table = Box::new([0; COMPONENT_TRANSFER_TABLE_LEN]);
    fill_component_transfer_channel(&mut table, 0, r);
    fill_component_transfer_channel(&mut table, 1, g);
    fill_component_transfer_channel(&mut table, 2, b);
    fill_component_transfer_channel(&mut table, 3, a);
    table
}

fn fill_component_transfer_channel(
    table: &mut ComponentTransferTable,
    channel: usize,
    transfer: &usvg::filter::TransferFunction,
) {
    let base = channel * COMPONENT_TRANSFER_TABLE_SIZE;
    for i in 0..COMPONENT_TRANSFER_TABLE_SIZE {
        let value = transfer_function_value(transfer, i as f32 / 255.0).clamp(0.0, 1.0);
        table[base + i] = (value * 255.0 + 0.5) as u32;
    }
}

fn transfer_function_value(transfer: &usvg::filter::TransferFunction, value: f32) -> f32 {
    match transfer {
        usvg::filter::TransferFunction::Identity => value,
        usvg::filter::TransferFunction::Table(values) => table_transfer_value(values, value),
        usvg::filter::TransferFunction::Discrete(values) => discrete_transfer_value(values, value),
        usvg::filter::TransferFunction::Linear { slope, intercept } => slope * value + intercept,
        usvg::filter::TransferFunction::Gamma {
            amplitude,
            exponent,
            offset,
        } => amplitude * value.powf(*exponent) + offset,
    }
}

fn table_transfer_value(values: &[f32], value: f32) -> f32 {
    match values.len() {
        0 => value,
        1 => values[0],
        len => {
            let position = value.clamp(0.0, 1.0) * (len - 1) as f32;
            let left = position.floor() as usize;
            let right = (left + 1).min(len - 1);
            lerp(values[left], values[right], position - left as f32)
        }
    }
}

fn discrete_transfer_value(values: &[f32], value: f32) -> f32 {
    match values.len() {
        0 => value,
        len => {
            values[(value.clamp(0.0, 1.0) * len as f32)
                .floor()
                .min((len - 1) as f32) as usize]
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn matrix_is_identity(values: &[f32]) -> bool {
    const IDENTITY: [f32; 20] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        1.0, 0.0,
    ];
    matrix_matches(values, &IDENTITY)
}

fn standard_grayscale_amount(values: &[f32]) -> Option<f32> {
    let amount = 1.0 - matrix_value(values, 0, 0).map(|value| (value - 0.2126) / 0.7874)?;
    let expected = [
        0.2126 + 0.7874 * (1.0 - amount),
        0.7152 - 0.7152 * (1.0 - amount),
        0.0722 - 0.0722 * (1.0 - amount),
        0.0,
        0.0,
        0.2126 - 0.2126 * (1.0 - amount),
        0.7152 + 0.2848 * (1.0 - amount),
        0.0722 - 0.0722 * (1.0 - amount),
        0.0,
        0.0,
        0.2126 - 0.2126 * (1.0 - amount),
        0.7152 - 0.7152 * (1.0 - amount),
        0.0722 + 0.9278 * (1.0 - amount),
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ];
    matrix_matches(values, &expected).then_some(amount.clamp(0.0, 1.0))
}

fn standard_sepia_amount(values: &[f32]) -> Option<f32> {
    let amount = 1.0 - matrix_value(values, 0, 0).map(|value| (value - 0.393) / 0.607)?;
    let expected = [
        0.393 + 0.607 * (1.0 - amount),
        0.769 - 0.769 * (1.0 - amount),
        0.189 - 0.189 * (1.0 - amount),
        0.0,
        0.0,
        0.349 - 0.349 * (1.0 - amount),
        0.686 + 0.314 * (1.0 - amount),
        0.168 - 0.168 * (1.0 - amount),
        0.0,
        0.0,
        0.272 - 0.272 * (1.0 - amount),
        0.534 - 0.534 * (1.0 - amount),
        0.131 + 0.869 * (1.0 - amount),
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ];
    matrix_matches(values, &expected).then_some(amount.clamp(0.0, 1.0))
}

fn matrix_value(values: &[f32], row: usize, column: usize) -> Option<f32> {
    values.get(row * 5 + column).copied()
}

fn matrix_matches(values: &[f32], expected: &[f32; 20]) -> bool {
    values.len() == expected.len()
        && values
            .iter()
            .zip(expected)
            .all(|(value, expected)| nearly_eq(*value, *expected))
}

fn equal_std_dev(x: f32, y: f32, feature: &str) -> Result<f32, SvgError> {
    if nearly_eq(x, y) {
        Ok(x)
    } else {
        Err(SvgError::unsupported(feature))
    }
}

fn nearly_eq(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.0e-4
}

fn tiny_path_to_bez(path: &usvg::tiny_skia_path::Path) -> BezPath {
    let mut out = BezPath::new();
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => out.move_to((p.x as f64, p.y as f64)),
            PathSegment::LineTo(p) => out.line_to((p.x as f64, p.y as f64)),
            PathSegment::QuadTo(p0, p1) => {
                out.quad_to((p0.x as f64, p0.y as f64), (p1.x as f64, p1.y as f64));
            }
            PathSegment::CubicTo(p0, p1, p2) => out.curve_to(
                (p0.x as f64, p0.y as f64),
                (p1.x as f64, p1.y as f64),
                (p2.x as f64, p2.y as f64),
            ),
            PathSegment::Close => out.close_path(),
        }
    }
    out
}

fn color_opacity_to_brush(color: usvg::Color, opacity: f32) -> Brush {
    Brush::Solid(Color::from_rgb8(color.red, color.green, color.blue).multiply_alpha(opacity))
}

fn color_to_rgb(color: usvg::Color) -> [f32; 3] {
    [
        color.red as f32 / 255.0,
        color.green as f32 / 255.0,
        color.blue as f32 / 255.0,
    ]
}

fn opacity_to_u8(opacity: f32) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn gradient_stops(stops: &[usvg::Stop], opacity: f32) -> Vec<ColorStop> {
    stops
        .iter()
        .map(|stop| {
            let color = stop.color();
            ColorStop::from((
                stop.offset().get(),
                Color::from_rgb8(color.red, color.green, color.blue)
                    .multiply_alpha(stop.opacity().get() * opacity),
            ))
        })
        .collect()
}

fn stroke_to_kurbo(stroke: &usvg::Stroke) -> Result<Stroke, SvgError> {
    if stroke.linejoin() == usvg::LineJoin::MiterClip {
        return Err(SvgError::unsupported("stroke-linejoin=miter-clip"));
    }

    let mut out = Stroke::new(stroke.width().get() as f64)
        .with_join(match stroke.linejoin() {
            usvg::LineJoin::Miter => Join::Miter,
            usvg::LineJoin::Round => Join::Round,
            usvg::LineJoin::Bevel => Join::Bevel,
            usvg::LineJoin::MiterClip => unreachable!(),
        })
        .with_miter_limit(stroke.miterlimit().get() as f64)
        .with_caps(match stroke.linecap() {
            usvg::LineCap::Butt => Cap::Butt,
            usvg::LineCap::Round => Cap::Round,
            usvg::LineCap::Square => Cap::Square,
        });

    if let Some(dasharray) = stroke.dasharray() {
        out = out.with_dashes(
            stroke.dashoffset() as f64,
            dasharray.iter().map(|dash| *dash as f64),
        );
    }
    Ok(out)
}

fn spread_method(method: SpreadMethod) -> Extend {
    match method {
        SpreadMethod::Pad => Extend::Pad,
        SpreadMethod::Reflect => Extend::Reflect,
        SpreadMethod::Repeat => Extend::Repeat,
    }
}

fn fill_rule(rule: usvg::FillRule) -> FillRule {
    match rule {
        usvg::FillRule::NonZero => FillRule::NonZero,
        usvg::FillRule::EvenOdd => FillRule::EvenOdd,
    }
}

fn blend_mode_to_mix(mode: usvg::BlendMode) -> Mix {
    match mode {
        usvg::BlendMode::Normal => Mix::Normal,
        usvg::BlendMode::Multiply => Mix::Multiply,
        usvg::BlendMode::Screen => Mix::Screen,
        usvg::BlendMode::Overlay => Mix::Overlay,
        usvg::BlendMode::Darken => Mix::Darken,
        usvg::BlendMode::Lighten => Mix::Lighten,
        usvg::BlendMode::ColorDodge => Mix::ColorDodge,
        usvg::BlendMode::ColorBurn => Mix::ColorBurn,
        usvg::BlendMode::HardLight => Mix::HardLight,
        usvg::BlendMode::SoftLight => Mix::SoftLight,
        usvg::BlendMode::Difference => Mix::Difference,
        usvg::BlendMode::Exclusion => Mix::Exclusion,
        usvg::BlendMode::Hue => Mix::Hue,
        usvg::BlendMode::Saturation => Mix::Saturation,
        usvg::BlendMode::Color => Mix::Color,
        usvg::BlendMode::Luminosity => Mix::Luminosity,
    }
}

fn transform_to_affine(transform: usvg::Transform) -> Affine {
    Affine::new([
        transform.sx as f64,
        transform.ky as f64,
        transform.kx as f64,
        transform.sy as f64,
        transform.tx as f64,
        transform.ty as f64,
    ])
}

fn affine_to_array(transform: Affine) -> [f32; 6] {
    transform.as_coeffs().map(|value| value as f32)
}

fn inverse_transform_array(
    transform: usvg::Transform,
    feature: &str,
) -> Result<[f32; 6], SvgError> {
    let Some(transform) = transform.invert() else {
        return Err(SvgError::unsupported(format!("non-invertible {feature}")));
    };
    Ok([
        transform.sx,
        transform.ky,
        transform.kx,
        transform.sy,
        transform.tx,
        transform.ty,
    ])
}

fn transform_region(region: Region, transform: Affine) -> Region {
    if transform == Affine::IDENTITY {
        return region;
    }
    match region {
        Region::Rect { rect, radius } => Region::rect(transform.transform_rect_bbox(rect), radius),
        Region::Path {
            path,
            transform: region_transform,
            tolerance,
        } => Region::path(path, transform * region_transform, tolerance),
    }
}

fn rect_path(rect: Rect) -> BezPath {
    rect.to_path(0.0)
}

fn nonzero_rect_to_kurbo(rect: usvg::NonZeroRect) -> Rect {
    Rect::new(
        rect.left() as f64,
        rect.top() as f64,
        rect.right() as f64,
        rect.bottom() as f64,
    )
}

fn nonzero_rect_to_bounds(rect: usvg::NonZeroRect) -> Bounds {
    Bounds::new(
        rect.left().floor() as i32,
        rect.top().floor() as i32,
        rect.right().ceil() as i32,
        rect.bottom().ceil() as i32,
    )
}

#[cfg(test)]
mod tests {
    use peniko::Color;

    use super::*;
    use crate::CpuRenderer;

    fn parse(svg: &str) -> usvg::Tree {
        usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap()
    }

    fn render(svg: &str, clear: Color) -> CpuRenderer {
        let tree = parse(svg);
        let size = tree.size();
        let mut scene = Scene::new(size.width().ceil() as u32, size.height().ceil() as u32);
        scene.push_svg(&tree).unwrap();
        let mut renderer = CpuRenderer::new(scene.width, scene.height, clear);
        renderer.render(&scene);
        renderer
    }

    fn assert_rgba_close(actual: [u8; 4], expected: [u8; 4], tolerance: u8) {
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| actual.abs_diff(expected) <= tolerance),
            "actual {actual:?}, expected {expected:?}"
        );
    }

    #[test]
    fn push_svg_renders_basic_fill_and_stroke() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">
                <rect x="4" y="4" width="18" height="18" fill="#ff0000" stroke="#0000ff" stroke-width="4"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(12, 12), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(4, 12), [0, 0, 255, 255]);
    }

    #[test]
    fn push_svg_keeps_group_opacity_isolated() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96">
                <g opacity="0.5">
                    <rect x="16" y="16" width="48" height="48" fill="#ff0000"/>
                    <rect x="32" y="32" width="48" height="48" fill="#ff0000"/>
                </g>
            </svg>"##,
            Color::WHITE,
        );

        let overlap = renderer.image().rgba8_at(40, 40);
        assert_eq!(overlap[0], 255);
        assert!(overlap[1].abs_diff(128) <= 1 && overlap[2].abs_diff(128) <= 1);
    }

    #[test]
    fn push_svg_renders_linear_gradient() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="4">
                <defs>
                    <linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="20" y2="0">
                        <stop offset="0" stop-color="#ff0000"/>
                        <stop offset="1" stop-color="#0000ff"/>
                    </linearGradient>
                </defs>
                <rect width="20" height="4" fill="url(#g)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        let left = renderer.image().rgba8_at(2, 2);
        let right = renderer.image().rgba8_at(18, 2);
        assert!(
            left[0] > left[2],
            "left pixel should be red-biased: {left:?}"
        );
        assert!(
            right[2] > right[0],
            "right pixel should be blue-biased: {right:?}"
        );
    }

    #[test]
    fn push_svg_renders_pattern_fill_with_opacity() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="4">
                <defs>
                    <pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4">
                        <rect width="2" height="4" fill="#ff0000"/>
                        <rect x="2" width="2" height="4" fill="#0000ff"/>
                    </pattern>
                </defs>
                <rect width="12" height="4" fill="url(#p)" fill-opacity="0.5"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_rgba_close(renderer.image().rgba8_at(1, 2), [128, 0, 0, 128], 1);
        assert_rgba_close(renderer.image().rgba8_at(3, 2), [0, 0, 128, 128], 1);
        assert_rgba_close(renderer.image().rgba8_at(5, 2), [128, 0, 0, 128], 1);
    }

    #[test]
    fn push_svg_applies_pattern_transform_before_repeating() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4">
                <defs>
                    <pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4" patternTransform="translate(2 0)">
                        <rect width="2" height="4" fill="#ff0000"/>
                        <rect x="2" width="2" height="4" fill="#0000ff"/>
                    </pattern>
                </defs>
                <rect width="8" height="4" fill="url(#p)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(1, 2), [0, 0, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(3, 2), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(5, 2), [0, 0, 255, 255]);
    }

    #[test]
    fn push_svg_renders_pattern_view_box() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4">
                <defs>
                    <pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4" viewBox="0 0 2 2" preserveAspectRatio="none">
                        <rect width="1" height="2" fill="#ff0000"/>
                        <rect x="1" width="1" height="2" fill="#0000ff"/>
                    </pattern>
                </defs>
                <rect width="8" height="4" fill="url(#p)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(1, 2), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(3, 2), [0, 0, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(5, 2), [255, 0, 0, 255]);
    }

    #[test]
    fn push_svg_renders_fe_gaussian_blur() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16">
                <defs>
                    <filter id="blur" x="0" y="0" width="32" height="16" filterUnits="userSpaceOnUse">
                        <feGaussianBlur stdDeviation="2"/>
                    </filter>
                </defs>
                <rect x="8" y="4" width="8" height="8" fill="#ff0000" filter="url(#blur)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        let center = renderer.image().rgba8_at(12, 8);
        let spread = renderer.image().rgba8_at(6, 8);
        assert!(
            center[0] > 0 && center[3] > 0,
            "blurred center should retain red coverage: {center:?}"
        );
        assert!(
            spread[0] > 0 && spread[3] > 0,
            "blur should spread outside the original rect: {spread:?}"
        );
    }

    #[test]
    fn push_svg_renders_fe_drop_shadow() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24">
                <defs>
                    <filter id="shadow" x="0" y="0" width="32" height="24" filterUnits="userSpaceOnUse">
                        <feDropShadow dx="8" dy="4" stdDeviation="0" flood-color="#0000ff"/>
                    </filter>
                </defs>
                <rect x="4" y="4" width="8" height="8" fill="#00ff00" filter="url(#shadow)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(6, 6), [0, 255, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(16, 10), [0, 0, 255, 255]);
    }

    #[test]
    fn push_svg_renders_fe_offset() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="16">
                <defs>
                    <filter id="offset" x="0" y="0" width="24" height="16" filterUnits="userSpaceOnUse">
                        <feOffset dx="6" dy="2"/>
                    </filter>
                </defs>
                <rect x="4" y="4" width="4" height="4" fill="#ff0000" filter="url(#offset)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(5, 5), [0, 0, 0, 0]);
        assert_eq!(renderer.image().rgba8_at(11, 7), [255, 0, 0, 255]);
    }

    #[test]
    fn push_svg_renders_fe_flood() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="flood" x="0" y="0" width="16" height="16" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#00ff00" flood-opacity="0.5"/>
                    </filter>
                </defs>
                <rect x="4" y="4" width="4" height="4" fill="#ff0000" filter="url(#flood)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(12, 12), [0, 128, 0, 128]);
    }

    #[test]
    fn push_svg_renders_fe_color_matrix() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="matrix" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feColorMatrix type="matrix" values="
                            0 0 0 0 0
                            0 0 0 0 0
                            1 0 0 0 0
                            0 0 0 1 0"/>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#ff0000" filter="url(#matrix)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(4, 4), [0, 0, 255, 255]);
    }

    #[test]
    fn push_svg_renders_fe_color_matrix_luminance_to_alpha() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="alpha" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feColorMatrix type="luminanceToAlpha"/>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#ff0000" filter="url(#alpha)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(4, 4), [0, 0, 0, 54]);
    }

    #[test]
    fn push_svg_renders_fe_component_transfer_mixed_types() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="transfer" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feComponentTransfer>
                            <feFuncR type="table" tableValues="0 1 0"/>
                            <feFuncG type="discrete" tableValues="1 0"/>
                            <feFuncB type="gamma" amplitude="1" exponent="2" offset="0"/>
                            <feFuncA type="linear" slope="0.5" intercept="0.25"/>
                        </feComponentTransfer>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#804020" filter="url(#transfer)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(4, 4), [190, 191, 3, 191]);
    }

    #[test]
    fn push_svg_renders_fe_blend_with_input_graph_and_subregion() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="blend" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#0000ff" result="blue"/>
                        <feBlend in="SourceGraphic" in2="blue" mode="multiply" x="0" y="0" width="4" height="8"/>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#ff0000" filter="url(#blend)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(2, 4), [0, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(6, 4), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_renders_fe_composite_with_source_alpha() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="mask" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#0000ff" result="blue"/>
                        <feComposite in="blue" in2="SourceAlpha" operator="in"/>
                    </filter>
                </defs>
                <rect width="4" height="8" fill="#ff0000" filter="url(#mask)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(2, 4), [0, 0, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(6, 4), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_renders_fe_composite_arithmetic() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="arith" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#0000ff" result="blue"/>
                        <feComposite in="SourceGraphic" in2="blue" operator="arithmetic" k1="0" k2="0.5" k3="0.5" k4="0"/>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#ff0000" filter="url(#arith)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(4, 4), [128, 0, 128, 255]);
    }

    #[test]
    fn push_svg_renders_fe_convolve_matrix() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="1">
                <defs>
                    <filter id="convolve" x="0" y="0" width="3" height="1" filterUnits="userSpaceOnUse">
                        <feConvolveMatrix order="3 1" targetX="1" targetY="0" edgeMode="duplicate" kernelMatrix="1 0 0"/>
                    </filter>
                </defs>
                <g filter="url(#convolve)">
                    <rect x="0" y="0" width="1" height="1" fill="#0a0000"/>
                    <rect x="1" y="0" width="1" height="1" fill="#140000"/>
                    <rect x="2" y="0" width="1" height="1" fill="#280000"/>
                </g>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(0, 0), [20, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(1, 0), [40, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(2, 0), [40, 0, 0, 255]);
    }

    #[test]
    fn push_svg_renders_fe_diffuse_lighting() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="1">
                <defs>
                    <filter id="diffuse" x="0" y="0" width="3" height="1" filterUnits="userSpaceOnUse">
                        <feDiffuseLighting surfaceScale="1" diffuseConstant="1" lighting-color="#ff0000">
                            <feDistantLight azimuth="180" elevation="0"/>
                        </feDiffuseLighting>
                    </filter>
                </defs>
                <g filter="url(#diffuse)">
                    <rect x="0" width="1" height="1" fill="#000000" fill-opacity="0"/>
                    <rect x="1" width="1" height="1" fill="#000000" fill-opacity="0.5"/>
                    <rect x="2" width="1" height="1" fill="#000000"/>
                </g>
            </svg>"##,
            Color::TRANSPARENT,
        );

        let center = renderer.image().rgba8_at(1, 0);
        assert!(
            center[0].abs_diff(180) <= 1 && center[1] == 0 && center[2] == 0 && center[3] == 255,
            "expected red diffuse lighting at alpha slope center, got {center:?}"
        );
    }

    #[test]
    fn push_svg_renders_fe_specular_lighting() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
                <defs>
                    <filter id="specular" x="0" y="0" width="1" height="1" filterUnits="userSpaceOnUse">
                        <feSpecularLighting in="SourceAlpha" surfaceScale="0" specularConstant="0.5" specularExponent="1" lighting-color="#ff8000">
                            <fePointLight x="0.5" y="0.5" z="1"/>
                        </feSpecularLighting>
                    </filter>
                </defs>
                <rect width="1" height="1" fill="#000000" filter="url(#specular)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(0, 0), [128, 64, 0, 128]);
    }

    #[test]
    fn push_svg_renders_fe_merge_in_graph_order() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="merge" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#0000ff" result="blue"/>
                        <feMerge x="0" y="0" width="4" height="8">
                            <feMergeNode in="blue"/>
                            <feMergeNode in="SourceGraphic"/>
                        </feMerge>
                    </filter>
                </defs>
                <rect width="8" height="8" fill="#ff0000" filter="url(#merge)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(2, 4), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(6, 4), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_renders_fe_morphology_dilate() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="dilate" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feMorphology operator="dilate" radius="1"/>
                    </filter>
                </defs>
                <rect x="3" y="3" width="2" height="2" fill="#ff0000" filter="url(#dilate)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(2, 3), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(5, 4), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_renders_fe_morphology_erode() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <defs>
                    <filter id="erode" x="0" y="0" width="8" height="8" filterUnits="userSpaceOnUse">
                        <feMorphology operator="erode" radius="1"/>
                    </filter>
                </defs>
                <rect x="2" y="2" width="4" height="4" fill="#ff0000" filter="url(#erode)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(3, 3), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(2, 3), [0, 0, 0, 0]);
        assert_eq!(renderer.image().rgba8_at(6, 3), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_preserves_css_filter_function_order() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                <rect width="8" height="8" fill="#202020" filter="brightness(200%) invert(100%)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        let px = renderer.image().rgba8_at(4, 4);
        assert!(
            px[0].abs_diff(191) <= 1 && px[1].abs_diff(191) <= 1 && px[2].abs_diff(191) <= 1,
            "brightness must run before invert: {px:?}"
        );
    }

    #[test]
    fn push_svg_unsupported_features_do_not_modify_scene() {
        let tree = parse(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="turbulence">
                        <feTurbulence baseFrequency="0.05"/>
                    </filter>
                </defs>
                <g filter="url(#turbulence)"><rect width="16" height="16" fill="#ff0000"/></g>
            </svg>"##,
        );
        let mut scene = Scene::new(16, 16);
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Color::from_rgb8(0, 0, 255),
            FillRule::NonZero,
        );

        let err = scene.push_svg(&tree).unwrap_err();
        assert_eq!(err.feature(), "feTurbulence");

        let mut renderer = CpuRenderer::new(16, 16, Color::TRANSPARENT);
        renderer.render(&scene);
        assert_eq!(renderer.image().rgba8_at(8, 8), [0, 0, 255, 255]);
    }
}
