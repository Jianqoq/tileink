use std::{collections::HashMap, error::Error, fmt, io::Cursor, sync::Arc};

use peniko::{
    Color, ColorStop, Compose, Extend, Gradient, Mix,
    kurbo::{Affine, BezPath, Cap, Join, Rect, Shape, Stroke},
};
use usvg::{Node, Paint, PaintOrder, SpreadMethod, tiny_skia_path::PathSegment};

use crate::{
    Brush, CpuRenderer, FillRule, Filter, Radius, Region, Scene,
    shared::{
        bounds::Bounds,
        brush::{PatternBrush, PatternSampling},
        image::{Image as RasterImage, rgba8_pack},
        layer::filter::{
            COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE, ComponentTransferTable,
            CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting, FilterInput,
            FilterPrimitive, FilterPrimitiveKind, LightSource, MorphologyOperator,
            SpecularLighting, Turbulence, TurbulenceKind,
        },
        layer::mask::{Mask as LayerMask, MaskKind},
        pixel::mul_div255,
    },
};

#[derive(Clone, Copy, Debug)]
pub struct SvgOptions {
    pub tolerance: f64,
    pub transform: Affine,
}

const MAX_PATTERN_DEPTH: u8 = 16;
const MAX_IMAGE_DEPTH: u8 = 16;

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            tolerance: 0.1,
            transform: Affine::IDENTITY,
        }
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
    image_depth: u8,
}

impl SvgBuilder {
    fn new(options: SvgOptions) -> Self {
        Self {
            base_transform: options.transform,
            options,
            pattern_depth: 0,
            image_depth: 0,
        }
    }

    fn push_tree(&self, scene: &mut Scene, tree: &usvg::Tree) -> Result<(), SvgError> {
        self.push_group(scene, tree.root())
    }

    fn push_group(&self, scene: &mut Scene, group: &usvg::Group) -> Result<(), SvgError> {
        let filter_layers = if group.filters().is_empty() {
            Vec::new()
        } else {
            let region_transform = self.base_transform * transform_to_affine(group.abs_transform());
            let content_transform = region_transform
                * inverse_affine(transform_to_affine(group.transform()), "filter transform")?;
            svg_filter_layers(
                self,
                group.filters(),
                region_transform,
                content_transform,
                scene.width,
                scene.height,
            )?
        };
        let mask_layer = group
            .mask()
            .map(|mask| self.svg_mask_layer(scene.width, scene.height, mask))
            .transpose()?;

        let mut pushed_layers = 0;
        if let Some(clip) = group.clip_path() {
            pushed_layers += self.push_clip_path_layers(scene, clip)?;
        }

        let layer_path = || {
            self.base_transform * rect_path(nonzero_rect_to_kurbo(group.abs_layer_bounding_box()))
        };
        if group.isolate()
            && group.opacity().get() >= 1.0
            && group.blend_mode() == usvg::BlendMode::Normal
            && mask_layer.is_none()
            && filter_layers.is_empty()
        {
            scene.push_isolate_layer(layer_path(), Affine::IDENTITY, self.options.tolerance);
            pushed_layers += 1;
        }
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
        if let Some((mask_scene, mask)) = mask_layer {
            scene.push_mask_layer(mask_scene, mask);
            pushed_layers += 1;
        }
        for layer in filter_layers.into_iter().rev() {
            scene.push_filter_layer(layer.filter, layer.region);
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

    fn svg_mask_layer(
        &self,
        width: u32,
        height: u32,
        mask: &usvg::Mask,
    ) -> Result<(Scene, LayerMask), SvgError> {
        let mut mask_scene = Scene::new(width, height);
        if let Some(parent) = mask.mask() {
            let (parent_scene, parent_mask) = self.svg_mask_layer(width, height, parent)?;
            mask_scene.push_mask_layer(parent_scene, parent_mask);
            self.push_group(&mut mask_scene, mask.root())?;
            mask_scene.pop_layer();
        } else {
            self.push_group(&mut mask_scene, mask.root())?;
        }
        Ok((
            mask_scene,
            LayerMask {
                region: transform_region(
                    Region::rect(nonzero_rect_to_kurbo(mask.rect()), Radius::all(0.0)),
                    self.base_transform,
                ),
                kind: match mask.kind() {
                    usvg::MaskType::Alpha => MaskKind::Alpha,
                    usvg::MaskType::Luminance => MaskKind::Luminance,
                },
            },
        ))
    }

    fn push_node(&self, scene: &mut Scene, node: &Node) -> Result<(), SvgError> {
        match node {
            Node::Group(group) => self.push_group(scene, group),
            Node::Path(path) => self.push_path(scene, path),
            Node::Text(text) => self.push_group(scene, text.flattened()),
            Node::Image(image) => self.push_image(scene, image),
        }
    }

    fn push_image(&self, scene: &mut Scene, image: &usvg::Image) -> Result<(), SvgError> {
        if !image.is_visible() {
            return Ok(());
        }

        let transform = self.base_transform * transform_to_affine(image.abs_transform());
        let size = image.size();
        let world_to_local = inverse_affine(transform, "image transform")?;
        let raster = match image.kind() {
            usvg::ImageKind::PNG(data) => decode_png_image(data)?,
            usvg::ImageKind::JPEG(data) => {
                decode_encoded_image(data, ::image::ImageFormat::Jpeg, "jpeg image")?
            }
            usvg::ImageKind::GIF(data) => {
                decode_encoded_image(data, ::image::ImageFormat::Gif, "gif image")?
            }
            usvg::ImageKind::WEBP(data) => {
                decode_encoded_image(data, ::image::ImageFormat::WebP, "webp image")?
            }
            usvg::ImageKind::SVG(tree) => {
                let (width, height) = svg_image_raster_size(transform, size);
                self.svg_image_to_raster(tree, width, height)?
            }
        };

        let brush = Brush::Pattern(PatternBrush {
            transform: affine_to_array(
                Affine::scale_non_uniform(
                    f64::from(raster.width) / f64::from(size.width()),
                    f64::from(raster.height) / f64::from(size.height()),
                ) * world_to_local,
            ),
            extend: Extend::Pad,
            sampling: image_sampling(image.rendering_mode()),
            opacity: 255,
            image: Arc::new(raster),
        });
        scene.push_path(
            rect_path(Rect::new(
                0.0,
                0.0,
                size.width() as f64,
                size.height() as f64,
            )),
            brush,
            transform,
            FillRule::NonZero,
            self.options.tolerance,
        );
        Ok(())
    }

    fn svg_image_to_raster(
        &self,
        tree: &usvg::Tree,
        width: u32,
        height: u32,
    ) -> Result<RasterImage, SvgError> {
        if self.image_depth >= MAX_IMAGE_DEPTH {
            return Err(SvgError::unsupported("recursive svg image"));
        }

        let size = tree.size();
        // SVG images are vector content. Rasterizing them at their intrinsic size and then
        // scaling the bitmap loses edge coverage when the image is enlarged by the outer SVG.
        let transform = Affine::scale_non_uniform(
            f64::from(width) / f64::from(size.width()),
            f64::from(height) / f64::from(size.height()),
        );
        let mut scene = Scene::new(width, height);
        SvgBuilder {
            options: SvgOptions {
                tolerance: self.options.tolerance,
                transform,
            },
            base_transform: transform,
            pattern_depth: self.pattern_depth,
            image_depth: self.image_depth + 1,
        }
        .push_tree(&mut scene, tree)?;

        let mut renderer = CpuRenderer::new(width, height, Color::TRANSPARENT);
        renderer.render(&scene);
        Ok(renderer.image().clone())
    }

    fn push_path(&self, scene: &mut Scene, path: &usvg::Path) -> Result<(), SvgError> {
        if !path.is_visible() {
            return Ok(());
        }

        let data = tiny_path_to_bez(path.data());
        let path_transform = transform_to_affine(path.abs_transform());
        let transform = self.base_transform * path_transform;
        match path.paint_order() {
            PaintOrder::FillAndStroke => {
                self.push_fill(scene, path, &data, path_transform, transform)?;
                self.push_stroke(scene, path, &data, path_transform, transform)?;
            }
            PaintOrder::StrokeAndFill => {
                self.push_stroke(scene, path, &data, path_transform, transform)?;
                self.push_fill(scene, path, &data, path_transform, transform)?;
            }
        }
        Ok(())
    }

    fn push_fill(
        &self,
        scene: &mut Scene,
        path: &usvg::Path,
        data: &BezPath,
        path_transform: Affine,
        transform: Affine,
    ) -> Result<(), SvgError> {
        let Some(fill) = path.fill() else {
            return Ok(());
        };
        if fill.opacity().get() <= 0.0 {
            return Ok(());
        }

        let brush = self.paint_to_brush(fill.paint(), fill.opacity().get(), path_transform)?;
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
        path_transform: Affine,
        transform: Affine,
    ) -> Result<(), SvgError> {
        let Some(stroke) = path.stroke() else {
            return Ok(());
        };
        if stroke.opacity().get() <= 0.0 {
            return Ok(());
        }

        let brush = self.paint_to_brush(stroke.paint(), stroke.opacity().get(), path_transform)?;
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

    fn paint_to_brush(
        &self,
        paint: &Paint,
        opacity: f32,
        path_transform: Affine,
    ) -> Result<Brush, SvgError> {
        match paint {
            Paint::Color(color) => Ok(color_opacity_to_brush(*color, opacity)),
            Paint::LinearGradient(source) => {
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
                linear.transform =
                    self.paint_server_inverse_transform(source.transform(), path_transform)?;
                Ok(brush)
            }
            Paint::RadialGradient(source) => {
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
                radial.transform =
                    self.paint_server_inverse_transform(source.transform(), path_transform)?;
                Ok(brush)
            }
            Paint::Pattern(pattern) => self.pattern_to_brush(pattern, opacity),
        }
    }

    fn paint_server_inverse_transform(
        &self,
        transform: usvg::Transform,
        path_transform: Affine,
    ) -> Result<[f32; 6], SvgError> {
        // usvg lowers paint-server units into the path's user space. Geometry is transformed
        // into scene coordinates before brush sampling, so fold both path and scene transforms
        // into the inverse.
        inverse_affine_to_array(
            self.base_transform * path_transform * transform_to_affine(transform),
            "gradientTransform",
        )
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
            image_depth: self.image_depth,
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
            extend: Extend::Repeat,
            sampling: PatternSampling::Nearest,
            opacity: opacity_to_u8(opacity),
        }))
    }

    fn filter_image_to_brush(
        &self,
        image: &usvg::filter::Image,
        filter_bounds: Bounds,
        primitive_rect: Rect,
        region_transform: Affine,
        content_transform: Affine,
    ) -> Result<Brush, SvgError> {
        if self.image_depth >= MAX_IMAGE_DEPTH {
            return Err(SvgError::unsupported("recursive feImage"));
        }

        let width = filter_bounds.width().max(1);
        let height = filter_bounds.height().max(1);
        let mut scene = Scene::new(width, height);
        if !filter_bounds.is_empty() {
            let buffer_origin =
                Affine::translate((-f64::from(filter_bounds.x0), -f64::from(filter_bounds.y0)));
            let local_transform = if fe_image_root_is_primitive_local_image(image.root()) {
                // External raster/SVG feImage content is already laid out by usvg
                // in primitive-subregion-local coordinates. The renderer filter
                // buffer is axis-aligned in scene pixels, so we place that local
                // image at the transformed primitive bbox and keep only the
                // filter-axis scale for child coordinates.
                let primitive_bounds = transform_rect_to_bounds(primitive_rect, region_transform);
                buffer_origin
                    * Affine::translate((
                        f64::from(primitive_bounds.x0),
                        f64::from(primitive_bounds.y0),
                    ))
                    * filter_axis_scale(region_transform)
            } else {
                // Internal href targets preserve their own document coordinates.
                // usvg does not move those roots into the primitive subregion, so
                // keep the existing content transform plus primitive origin.
                buffer_origin
                    * content_transform
                    * Affine::translate((primitive_rect.x0, primitive_rect.y0))
            };
            SvgBuilder {
                options: SvgOptions {
                    tolerance: self.options.tolerance,
                    transform: local_transform,
                },
                base_transform: local_transform,
                pattern_depth: self.pattern_depth,
                image_depth: self.image_depth + 1,
            }
            .push_group(&mut scene, image.root())?;
        }

        let mut renderer = CpuRenderer::new(width, height, Color::TRANSPARENT);
        renderer.render(&scene);
        Ok(Brush::Pattern(PatternBrush {
            image: Arc::new(renderer.image().clone()),
            transform: affine_to_array(Affine::translate((
                -f64::from(filter_bounds.x0),
                -f64::from(filter_bounds.y0),
            ))),
            extend: Extend::Pad,
            sampling: PatternSampling::Nearest,
            opacity: 255,
        }))
    }
}

struct SvgFilterLayer {
    filter: Filter,
    region: Region,
}

fn svg_filter_layers(
    builder: &SvgBuilder,
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
    filter: &usvg::filter::Filter,
    region_transform: Affine,
    content_transform: Affine,
    width: u32,
    height: u32,
) -> Result<Option<SvgFilterLayer>, SvgError> {
    let filter_rect = nonzero_rect_to_kurbo(filter.rect());
    let filter_bounds = transform_rect_to_bounds(filter_rect, region_transform)
        .intersect(Bounds::canvas(width, height));
    let value_transform = filter_axis_scale(region_transform);
    let mut primitives = Vec::new();
    let mut source_regions = Vec::new();
    let mut results = HashMap::new();
    for primitive in filter.primitives() {
        let index = primitives.len();
        let (filter_primitive, source_region) = svg_filter_primitive(
            primitive,
            SvgFilterPrimitiveContext {
                builder,
                results: &results,
                source_regions: &source_regions,
                region_transform,
                content_transform,
                value_transform,
                filter_bounds,
            },
        )?;
        primitives.push(filter_primitive);
        source_regions.push(source_region);
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
            region: transform_region(
                Region::rect(filter_rect, Radius::all(0.0)),
                region_transform,
            ),
        }))
    }
}

struct SvgFilterPrimitiveContext<'a> {
    builder: &'a SvgBuilder,
    results: &'a HashMap<String, usize>,
    source_regions: &'a [Bounds],
    region_transform: Affine,
    content_transform: Affine,
    value_transform: Affine,
    filter_bounds: Bounds,
}

fn svg_filter_primitive(
    primitive: &usvg::filter::Primitive,
    ctx: SvgFilterPrimitiveContext<'_>,
) -> Result<(FilterPrimitive, Bounds), SvgError> {
    let primitive_rect = nonzero_rect_to_kurbo(primitive.rect());
    let region = transform_rect_to_bounds(primitive_rect, ctx.region_transform);
    let (input, input2, kind) = match primitive.kind() {
        usvg::filter::Kind::GaussianBlur(blur) => {
            let (radius_x, radius_y) = transform_filter_radii(
                ctx.value_transform,
                blur.std_dev_x().get(),
                blur.std_dev_y().get(),
            );
            let filter = Filter::Blur { radius_x, radius_y };
            (
                svg_filter_input(blur.input(), ctx.results, "feGaussianBlur")?,
                None,
                FilterPrimitiveKind::Filter(Box::new(filter)),
            )
        }
        usvg::filter::Kind::DropShadow(shadow) => {
            let (offset_x, offset_y) =
                transform_filter_vector(ctx.value_transform, shadow.dx(), shadow.dy());
            let (radius_x, radius_y) = transform_filter_radii(
                ctx.value_transform,
                shadow.std_dev_x().get(),
                shadow.std_dev_y().get(),
            );
            let filter = Filter::DropShadow {
                offset_x,
                offset_y,
                radius: equal_std_dev(radius_x, radius_y, "anisotropic feDropShadow")?,
                brush: color_opacity_to_brush(shadow.color(), shadow.opacity().get()),
            };
            (
                svg_filter_input(shadow.input(), ctx.results, "feDropShadow")?,
                None,
                FilterPrimitiveKind::Filter(Box::new(filter)),
            )
        }
        usvg::filter::Kind::ColorMatrix(matrix) => {
            let kind = filter_to_primitive_kind(color_matrix_to_filter(matrix.kind())?);
            (
                svg_filter_input(matrix.input(), ctx.results, "feColorMatrix")?,
                None,
                kind,
            )
        }
        usvg::filter::Kind::ComponentTransfer(transfer) => {
            let kind = filter_to_primitive_kind(component_transfer_to_filter(transfer)?);
            (
                svg_filter_input(transfer.input(), ctx.results, "feComponentTransfer")?,
                None,
                kind,
            )
        }
        usvg::filter::Kind::Blend(blend) => (
            svg_filter_input(blend.input1(), ctx.results, "feBlend")?,
            Some(svg_filter_input(blend.input2(), ctx.results, "feBlend")?),
            FilterPrimitiveKind::Blend {
                mode: blend_mode_to_mix(blend.mode()),
            },
        ),
        usvg::filter::Kind::Composite(composite) => (
            svg_filter_input(composite.input1(), ctx.results, "feComposite")?,
            Some(svg_filter_input(
                composite.input2(),
                ctx.results,
                "feComposite",
            )?),
            FilterPrimitiveKind::Composite {
                operator: composite_operator(composite.operator()),
            },
        ),
        usvg::filter::Kind::ConvolveMatrix(convolve) => (
            svg_filter_input(convolve.input(), ctx.results, "feConvolveMatrix")?,
            None,
            FilterPrimitiveKind::Filter(Box::new(convolve_matrix_to_filter(convolve))),
        ),
        usvg::filter::Kind::DiffuseLighting(lighting) => (
            svg_filter_input(lighting.input(), ctx.results, "feDiffuseLighting")?,
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
        usvg::filter::Kind::Image(image) => (
            FilterInput::SourceGraphic,
            None,
            FilterPrimitiveKind::Image {
                brush: ctx.builder.filter_image_to_brush(
                    image,
                    ctx.filter_bounds,
                    primitive_rect,
                    ctx.region_transform,
                    ctx.content_transform,
                )?,
            },
        ),
        usvg::filter::Kind::Merge(merge) => (
            FilterInput::SourceGraphic,
            None,
            FilterPrimitiveKind::Merge {
                inputs: svg_filter_inputs(merge.inputs(), ctx.results, "feMerge")?,
            },
        ),
        usvg::filter::Kind::Morphology(morphology) => (
            svg_filter_input(morphology.input(), ctx.results, "feMorphology")?,
            None,
            FilterPrimitiveKind::Filter(Box::new(Filter::Morphology {
                radius_x: transform_filter_radius_x(
                    ctx.value_transform,
                    morphology.radius_x().get(),
                ),
                radius_y: transform_filter_radius_y(
                    ctx.value_transform,
                    morphology.radius_y().get(),
                ),
                operator: morphology_operator(morphology.operator()),
            })),
        ),
        usvg::filter::Kind::Offset(offset) => {
            let (dx, dy) = transform_filter_vector(ctx.value_transform, offset.dx(), offset.dy());
            (
                svg_filter_input(offset.input(), ctx.results, "feOffset")?,
                None,
                FilterPrimitiveKind::Filter(Box::new(Filter::Offset { dx, dy })),
            )
        }
        usvg::filter::Kind::SpecularLighting(lighting) => (
            svg_filter_input(lighting.input(), ctx.results, "feSpecularLighting")?,
            None,
            FilterPrimitiveKind::Filter(Box::new(specular_lighting_to_filter(lighting))),
        ),
        usvg::filter::Kind::Tile(tile) => {
            let input = svg_filter_input(tile.input(), ctx.results, "feTile")?;
            (
                input,
                None,
                FilterPrimitiveKind::Tile {
                    source_region: svg_filter_input_source_region(
                        input,
                        ctx.source_regions,
                        ctx.filter_bounds,
                    ),
                },
            )
        }
        usvg::filter::Kind::Turbulence(turbulence) => (
            FilterInput::SourceGraphic,
            None,
            FilterPrimitiveKind::Turbulence(turbulence_to_filter(
                turbulence,
                primitive.color_interpolation(),
                ctx.region_transform,
                ctx.filter_bounds,
            )),
        ),
    };
    let source_region = svg_filter_output_source_region(
        primitive,
        input,
        input2,
        &kind,
        region,
        SvgFilterSourceContext {
            source_regions: ctx.source_regions,
            filter_bounds: ctx.filter_bounds,
            value_transform: ctx.value_transform,
        },
    );
    Ok((
        FilterPrimitive {
            input,
            input2,
            region,
            kind,
        },
        source_region,
    ))
}

fn filter_to_primitive_kind(filter: Option<Filter>) -> FilterPrimitiveKind {
    match filter {
        Some(filter) => FilterPrimitiveKind::Filter(Box::new(filter)),
        None => FilterPrimitiveKind::Identity,
    }
}

#[derive(Clone, Copy)]
struct SvgFilterSourceContext<'a> {
    source_regions: &'a [Bounds],
    filter_bounds: Bounds,
    value_transform: Affine,
}

fn svg_filter_output_source_region(
    primitive: &usvg::filter::Primitive,
    input: FilterInput,
    input2: Option<FilterInput>,
    kind: &FilterPrimitiveKind,
    region: Bounds,
    ctx: SvgFilterSourceContext<'_>,
) -> Bounds {
    let region = region.intersect(ctx.filter_bounds);
    match primitive.kind() {
        usvg::filter::Kind::Flood(_)
        | usvg::filter::Kind::Image(_)
        | usvg::filter::Kind::Turbulence(_) => region,
        usvg::filter::Kind::Offset(_) => {
            svg_filter_input_source_region(input, ctx.source_regions, ctx.filter_bounds)
                .intersect(region)
        }
        usvg::filter::Kind::GaussianBlur(blur) => {
            let (radius_x, radius_y) = transform_filter_radii(
                ctx.value_transform,
                blur.std_dev_x().get(),
                blur.std_dev_y().get(),
            );
            svg_filter_input_source_region(input, ctx.source_regions, ctx.filter_bounds)
                .outset(blur_outset(radius_x.max(radius_y)))
                .intersect(region)
        }
        usvg::filter::Kind::Morphology(morphology) => {
            let input_region =
                svg_filter_input_source_region(input, ctx.source_regions, ctx.filter_bounds);
            let radius = match morphology.operator() {
                usvg::filter::MorphologyOperator::Dilate => {
                    transform_filter_radius_x(ctx.value_transform, morphology.radius_x().get())
                        .max(transform_filter_radius_y(
                            ctx.value_transform,
                            morphology.radius_y().get(),
                        ))
                        .max(0.0)
                        .ceil() as i32
                }
                usvg::filter::MorphologyOperator::Erode => 0,
            };
            input_region.outset(radius).intersect(region)
        }
        usvg::filter::Kind::Blend(_) => {
            svg_filter_input_source_region(input, ctx.source_regions, ctx.filter_bounds)
                .union(svg_filter_input_source_region(
                    input2.expect("feBlend lowering produced no second input"),
                    ctx.source_regions,
                    ctx.filter_bounds,
                ))
                .intersect(region)
        }
        usvg::filter::Kind::Composite(composite) => {
            let input_region =
                svg_filter_input_source_region(input, ctx.source_regions, ctx.filter_bounds);
            let input2_region = svg_filter_input_source_region(
                input2.expect("feComposite lowering produced no second input"),
                ctx.source_regions,
                ctx.filter_bounds,
            );
            match composite.operator() {
                usvg::filter::CompositeOperator::In => input_region.intersect(input2_region),
                usvg::filter::CompositeOperator::Out => input_region,
                _ => input_region.union(input2_region),
            }
            .intersect(region)
        }
        usvg::filter::Kind::Merge(_) => match kind {
            FilterPrimitiveKind::Merge { inputs } => inputs
                .iter()
                .map(|input| {
                    svg_filter_input_source_region(*input, ctx.source_regions, ctx.filter_bounds)
                })
                .fold(Bounds::new(0, 0, 0, 0), BoundsExt::union)
                .intersect(region),
            _ => region,
        },
        usvg::filter::Kind::Tile(_) => match kind {
            FilterPrimitiveKind::Tile { source_region } if !source_region.is_empty() => region,
            _ => Bounds::new(0, 0, 0, 0),
        },
        _ => svg_filter_input_source_region(input, ctx.source_regions, ctx.filter_bounds)
            .intersect(region),
    }
}

trait BoundsExt {
    fn union(self, other: Bounds) -> Bounds;
}

impl BoundsExt for Bounds {
    fn union(self, other: Bounds) -> Bounds {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Bounds::new(
            self.x0.min(other.x0),
            self.y0.min(other.y0),
            self.x1.max(other.x1),
            self.y1.max(other.y1),
        )
    }
}

fn svg_filter_input_source_region(
    input: FilterInput,
    source_regions: &[Bounds],
    filter_bounds: Bounds,
) -> Bounds {
    match input {
        FilterInput::SourceGraphic | FilterInput::SourceAlpha => filter_bounds,
        FilterInput::Primitive(index) => source_regions[index],
    }
}

fn blur_outset(radius: f32) -> i32 {
    (radius.max(0.0) * 3.0).ceil() as i32
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

fn turbulence_to_filter(
    turbulence: &usvg::filter::Turbulence,
    color_interpolation: usvg::filter::ColorInterpolation,
    transform: Affine,
    filter_bounds: Bounds,
) -> Turbulence {
    let [a, b, c, d, e, f] = transform.as_coeffs();
    Turbulence {
        base_frequency_x: turbulence.base_frequency_x().get(),
        base_frequency_y: turbulence.base_frequency_y().get(),
        num_octaves: turbulence.num_octaves(),
        seed: turbulence.seed(),
        stitch_tiles: turbulence.stitch_tiles(),
        kind: turbulence_kind(turbulence.kind()),
        linear_rgb: color_interpolation == usvg::filter::ColorInterpolation::LinearRGB,
        transform_x: e as f32,
        transform_y: f as f32,
        scale_x: a.hypot(c) as f32,
        scale_y: b.hypot(d) as f32,
        tile_x: filter_bounds.x0 as f32,
        tile_y: filter_bounds.y0 as f32,
        tile_width: filter_bounds.width() as f32,
        tile_height: filter_bounds.height() as f32,
    }
}

fn turbulence_kind(kind: usvg::filter::TurbulenceKind) -> TurbulenceKind {
    match kind {
        usvg::filter::TurbulenceKind::Turbulence => TurbulenceKind::Turbulence,
        usvg::filter::TurbulenceKind::FractalNoise => TurbulenceKind::FractalNoise,
    }
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

fn decode_png_image(data: &[u8]) -> Result<RasterImage, SvgError> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|err| SvgError::unsupported(format!("invalid PNG image: {err}")))?;
    let mut bytes = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut bytes)
        .map_err(|err| SvgError::unsupported(format!("invalid PNG image: {err}")))?;
    let bytes = &bytes[..info.buffer_size()];

    let pixels = match info.color_type {
        png::ColorType::Rgba => bytes
            .chunks_exact(4)
            .map(|px| premul_rgba8_pack(px[0], px[1], px[2], px[3]))
            .collect(),
        png::ColorType::Rgb => bytes
            .chunks_exact(3)
            .map(|px| premul_rgba8_pack(px[0], px[1], px[2], 255))
            .collect(),
        png::ColorType::Grayscale => bytes
            .iter()
            .map(|&gray| premul_rgba8_pack(gray, gray, gray, 255))
            .collect(),
        png::ColorType::GrayscaleAlpha => bytes
            .chunks_exact(2)
            .map(|px| premul_rgba8_pack(px[0], px[0], px[0], px[1]))
            .collect(),
        png::ColorType::Indexed => {
            return Err(SvgError::unsupported("indexed PNG image"));
        }
    };

    Ok(RasterImage {
        width: info.width,
        height: info.height,
        pixels,
    })
}

fn premul_rgba8_pack(r: u8, g: u8, b: u8, a: u8) -> u32 {
    rgba8_pack([mul_div255(r, a), mul_div255(g, a), mul_div255(b, a), a])
}

fn decode_encoded_image(
    data: &[u8],
    format: ::image::ImageFormat,
    feature: &str,
) -> Result<RasterImage, SvgError> {
    let image = ::image::load_from_memory_with_format(data, format)
        .map_err(|err| SvgError::unsupported(format!("invalid {feature}: {err}")))?
        .into_rgba8();
    let (width, height) = image.dimensions();
    let pixels = image
        .pixels()
        .map(|px| premul_rgba8_pack(px.0[0], px.0[1], px.0[2], px.0[3]))
        .collect();
    Ok(RasterImage {
        width,
        height,
        pixels,
    })
}

fn svg_image_raster_size(transform: Affine, size: usvg::Size) -> (u32, u32) {
    let [xx, yx, xy, yy, _, _] = transform.as_coeffs();
    let scale_x = xx.hypot(yx).max(f64::EPSILON);
    let scale_y = xy.hypot(yy).max(f64::EPSILON);
    (
        (f64::from(size.width()) * scale_x).ceil().max(1.0) as u32,
        (f64::from(size.height()) * scale_y).ceil().max(1.0) as u32,
    )
}

fn image_sampling(rendering: usvg::ImageRendering) -> PatternSampling {
    // SVG raster images are smooth by default; explicit speed/crisp/pixelated hints keep hard edges.
    match rendering {
        usvg::ImageRendering::OptimizeSpeed
        | usvg::ImageRendering::CrispEdges
        | usvg::ImageRendering::Pixelated => PatternSampling::Nearest,
        usvg::ImageRendering::OptimizeQuality
        | usvg::ImageRendering::Smooth
        | usvg::ImageRendering::HighQuality => PatternSampling::Bilinear,
    }
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

fn inverse_affine(transform: Affine, feature: &str) -> Result<Affine, SvgError> {
    let determinant = transform.determinant();
    if !determinant.is_finite() || determinant.abs() <= f64::EPSILON {
        return Err(SvgError::unsupported(format!("non-invertible {feature}")));
    }
    let inverse = transform.inverse();
    if !inverse.as_coeffs().iter().all(|value| value.is_finite()) {
        return Err(SvgError::unsupported(format!("non-invertible {feature}")));
    }
    Ok(inverse)
}

fn inverse_affine_to_array(transform: Affine, feature: &str) -> Result<[f32; 6], SvgError> {
    inverse_affine(transform, feature).map(affine_to_array)
}

fn fe_image_root_is_primitive_local_image(root: &usvg::Group) -> bool {
    let [Node::Group(group)] = root.children() else {
        return false;
    };
    is_generated_image_group_id(group.id())
        && group
            .children()
            .iter()
            .all(|child| matches!(child, Node::Image(_)))
}

fn is_generated_image_group_id(id: &str) -> bool {
    id.strip_prefix("image").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn filter_axis_scale(transform: Affine) -> Affine {
    let [a, b, c, d, _, _] = transform.as_coeffs();
    // Filter primitive values are applied in the axis-aligned filter buffer.
    // Preserve scene scale, but do not let skew/rotation mix `dx` into `dy`.
    Affine::scale_non_uniform(a.hypot(c), b.hypot(d))
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

fn transform_filter_vector(transform: Affine, x: f32, y: f32) -> (f32, f32) {
    // SVG filter lengths are in user space, while renderer filter kernels run in scene pixels.
    let [a, b, c, d, _, _] = transform.as_coeffs();
    (
        (a * x as f64 + c * y as f64) as f32,
        (b * x as f64 + d * y as f64) as f32,
    )
}

fn transform_filter_radii(transform: Affine, x: f32, y: f32) -> (f32, f32) {
    (
        transform_filter_radius_x(transform, x),
        transform_filter_radius_y(transform, y),
    )
}

fn transform_filter_radius_x(transform: Affine, radius: f32) -> f32 {
    let (x, y) = transform_filter_vector(transform, radius, 0.0);
    x.hypot(y)
}

fn transform_filter_radius_y(transform: Affine, radius: f32) -> f32 {
    let (x, y) = transform_filter_vector(transform, 0.0, radius);
    x.hypot(y)
}

fn transform_rect_to_bounds(rect: Rect, transform: Affine) -> Bounds {
    rect_to_bounds(if transform == Affine::IDENTITY {
        rect
    } else {
        transform.transform_rect_bbox(rect)
    })
}

fn rect_to_bounds(rect: Rect) -> Bounds {
    Bounds::new(
        rect.x0.floor() as i32,
        rect.y0.floor() as i32,
        rect.x1.ceil() as i32,
        rect.y1.ceil() as i32,
    )
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
        render_tree_with_options(
            &tree,
            clear,
            SvgOptions::default(),
            size.width().ceil() as u32,
            size.height().ceil() as u32,
        )
    }

    fn render_with_options(
        svg: &str,
        clear: Color,
        options: SvgOptions,
        width: u32,
        height: u32,
    ) -> CpuRenderer {
        let tree = parse(svg);
        render_tree_with_options(&tree, clear, options, width, height)
    }

    fn render_tree_with_options(
        tree: &usvg::Tree,
        clear: Color,
        options: SvgOptions,
        width: u32,
        height: u32,
    ) -> CpuRenderer {
        let mut scene = Scene::new(width, height);
        scene.push_svg_with_options(&tree, options).unwrap();
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

    const OPAQUE_RED_BLUE_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=";
    const TRANSLUCENT_ORANGE_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP438DQAAAGAQIADTyPKQAAAABJRU5ErkJggg==";

    fn encoded_test_image(format: ::image::ImageFormat) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        let image = ::image::RgbImage::from_raw(2, 1, vec![255, 0, 0, 0, 0, 255]).unwrap();
        ::image::DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, format)
            .unwrap();
        bytes.into_inner()
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
    fn push_svg_renders_line_with_default_start_coordinates() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
                <path d="M 0 0 L 160 180" stroke="red" stroke-width="4"/>
                <line x2="160" y2="180" stroke="green" stroke-width="4"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        for (x, y) in [(32, 36), (80, 90), (128, 144)] {
            let px = renderer.image().rgba8_at(x, y);
            assert!(
                px[1] > px[0] && px[1] > 0,
                "expected green line coverage at ({x}, {y}), got {px:?}"
            );
        }
        assert_eq!(
            renderer.image().rgba8_at(80, 4),
            [0, 0, 0, 0],
            "the clipped stroke cap must not become a full-width top tile row"
        );
    }

    #[test]
    fn push_svg_renders_line_with_default_y2_coordinate_without_endpoint_tile_fill() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
                <path d="M 20 40 L 160 0" stroke="red"/>
                <line x1="20" y1="40" x2="160" stroke="green"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        let covered = renderer.image().rgba8_at(90, 20);
        assert!(
            covered[1] > 0,
            "expected green line coverage, got {covered:?}"
        );
        assert_eq!(
            renderer.image().rgba8_at(170, 8),
            [0, 0, 0, 0],
            "line endpoint must not fill the endpoint tile"
        );
    }

    #[test]
    fn push_svg_renders_top_clipped_circle_without_double_top_backdrop() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="300" viewBox="0 0 200 200">
                <circle cx="100" r="80" fill="green"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert!(
            renderer.image().rgba8_at(260, 8)[1] > 0,
            "expected the top-clipped circle body to cover the right edge tile"
        );
        assert_eq!(
            renderer.image().rgba8_at(271, 8),
            [0, 0, 0, 0],
            "top-clipped circle edge must not fill the whole right edge tile"
        );
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
    fn push_svg_keeps_opacity_isolated_around_filtered_child() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feOffset in="SourceGraphic" dx="0" dy="0"/>
                    </filter>
                </defs>
                <g opacity="0.5">
                    <rect width="16" height="16" fill="#008000"/>
                    <g filter="url(#f)">
                        <rect width="16" height="16" fill="#0000ff"/>
                    </g>
                </g>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(8, 8), [0, 0, 128, 128]);
    }

    #[test]
    fn push_svg_keeps_blend_isolated_around_filtered_child() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feOffset in="SourceGraphic" dx="0" dy="0"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#808080"/>
                <g style="mix-blend-mode:multiply">
                    <rect width="16" height="16" fill="#ff0000"/>
                    <g filter="url(#f)">
                        <rect width="16" height="16" fill="#00ff00"/>
                    </g>
                </g>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(8, 8), [0, 128, 0, 255]);
    }

    #[test]
    fn push_svg_renders_fe_image_href_with_primitive_xy() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <rect id="src" width="8" height="8" fill="#008000"/>
                    <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feImage href="#src" x="4" y="4" width="8" height="8"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#ff0000" filter="url(#f)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(6, 6), [0, 128, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(2, 2), [0, 0, 0, 0]);
        assert_eq!(renderer.image().rgba8_at(13, 13), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_fe_image_result_can_feed_composite() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4">
                <defs>
                    <rect id="src" width="4" height="4" fill="#008000"/>
                    <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="8" height="4">
                        <feImage href="#src" x="0" y="0" width="4" height="4" result="img"/>
                        <feComposite in="img" in2="SourceAlpha" operator="in"/>
                    </filter>
                </defs>
                <rect x="2" width="4" height="4" fill="#ff0000" filter="url(#f)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(1, 2), [0, 0, 0, 0]);
        assert_eq!(renderer.image().rgba8_at(2, 2), [0, 128, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(3, 2), [0, 128, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(4, 2), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_fe_image_tracks_filtered_element_transform() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <rect id="src" width="16" height="16" fill="#008000"/>
                    <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feImage href="#src"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#ff0000" filter="url(#f)" transform="scale(0.5)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(6, 6), [0, 128, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(10, 6), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_places_external_fe_image_in_transformed_primitive_subregion() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="80">
                <defs>
                    <filter id="f" x="0" y="0" width="1" height="1">
                        <feImage x="20" width="20"
                            href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='1' height='1'%3E%3Crect width='1' height='1' fill='%23008000'/%3E%3C/svg%3E"/>
                    </filter>
                </defs>
                <rect x="20" y="20" width="40" height="40" fill="#ff0000"
                      filter="url(#f)" transform="rotate(45 40 40)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(16, 26), [0, 128, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(36, 46), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_supports_isolated_group_without_opacity_or_blend() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <rect width="16" height="16" fill="#808080"/>
                <g style="isolation:isolate">
                    <rect width="16" height="16" fill="#ff0000" style="mix-blend-mode:multiply"/>
                </g>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(8, 8), [255, 0, 0, 255]);
    }

    #[test]
    fn push_svg_supports_alpha_mask() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <mask id="m" mask-type="alpha" maskUnits="userSpaceOnUse" x="0" y="0" width="8" height="16">
                        <rect width="16" height="16" fill="#ffffff" fill-opacity="0.5"/>
                    </mask>
                </defs>
                <rect width="16" height="16" fill="#ff0000" mask="url(#m)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(4, 8), [128, 0, 0, 128]);
        assert_eq!(renderer.image().rgba8_at(12, 8), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_supports_luminance_mask() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <rect width="16" height="16" fill="#ff0000"/>
                    </mask>
                </defs>
                <rect width="16" height="16" fill="#00ff00" mask="url(#m)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        let px = renderer.image().rgba8_at(8, 8);
        assert!(
            px[1].abs_diff(54) <= 1 && px[3].abs_diff(54) <= 1,
            "got {px:?}"
        );
    }

    #[test]
    fn push_svg_renders_png_image_with_transform() {
        let renderer = render(
            &format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
                    <image href="data:image/png;base64,{OPAQUE_RED_BLUE_PNG}" width="4" height="2" preserveAspectRatio="none" image-rendering="optimizeSpeed"/>
                </svg>"##
            ),
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(1, 1), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(2, 1), [0, 0, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(3, 1), [0, 0, 255, 255]);
    }

    #[test]
    fn push_svg_smooths_raster_image_by_default() {
        let renderer = render(
            &format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
                    <image href="data:image/png;base64,{OPAQUE_RED_BLUE_PNG}" width="4" height="2" preserveAspectRatio="none"/>
                </svg>"##
            ),
            Color::TRANSPARENT,
        );

        let edge = renderer.image().rgba8_at(2, 1);
        assert_eq!(edge[3], 255);
        assert!(
            edge[0] > 0 && edge[2] > 0,
            "expected smoothed red/blue edge, got {edge:?}"
        );
    }

    #[test]
    fn push_svg_uses_nearest_sampling_for_image_rendering_hint() {
        let renderer = render(
            &format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
                    <image href="data:image/png;base64,{OPAQUE_RED_BLUE_PNG}" width="4" height="2" preserveAspectRatio="none" style="image-rendering:pixelated"/>
                </svg>"##
            ),
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(1, 1), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(2, 1), [0, 0, 255, 255]);
    }

    #[test]
    fn push_svg_decodes_png_image_into_premultiplied_pixels() {
        let renderer = render(
            &format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
                    <image href="data:image/png;base64,{TRANSLUCENT_ORANGE_PNG}" width="1" height="1"/>
                </svg>"##
            ),
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(0, 0), [128, 64, 0, 128]);
    }

    #[test]
    fn push_svg_renders_embedded_svg_image() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
                <image href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='2' height='1'%3E%3Crect width='1' height='1' fill='%23ff0000'/%3E%3Crect x='1' width='1' height='1' fill='%230000ff'/%3E%3C/svg%3E"
                       width="4" height="2" preserveAspectRatio="none"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(1, 1), [255, 0, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(3, 1), [0, 0, 255, 255]);
    }

    #[test]
    fn push_svg_renders_scaled_sliced_embedded_svg_image_top_tile() {
        let renderer = render_with_options(
            r##"<svg width="200" height="200" viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
                <image x="36" y="3" width="128" height="64"
                       href="data:image/svg+xml,%3Csvg viewBox='0 0 20 20' xmlns='http://www.w3.org/2000/svg'%3E%3Crect fill='%2300f' height='20' rx='5' width='20'/%3E%3Crect fill='none' height='16' rx='4' stroke='%230f0' width='16' x='2' y='2'/%3E%3C/svg%3E"
                       preserveAspectRatio="xMaxYMax slice"/>
            </svg>"##,
            Color::TRANSPARENT,
            SvgOptions {
                transform: Affine::scale(1.5),
                ..Default::default()
            },
            300,
            300,
        );

        assert_eq!(renderer.image().rgba8_at(100, 6), [0, 0, 255, 255]);
        assert_eq!(renderer.image().rgba8_at(100, 18), [0, 0, 255, 255]);
    }

    #[test]
    fn svg_image_raster_size_includes_outer_transform_scale() {
        let size = usvg::Size::from_wh(100.0, 100.0).unwrap();

        assert_eq!(svg_image_raster_size(Affine::scale(2.4), size), (240, 240));
    }

    #[test]
    fn push_svg_decodes_common_raster_image_formats() {
        for format in [
            ::image::ImageFormat::Gif,
            ::image::ImageFormat::Jpeg,
            ::image::ImageFormat::WebP,
        ] {
            let raster = decode_encoded_image(&encoded_test_image(format), format, "test image")
                .unwrap_or_else(|err| panic!("{format:?}: {err}"));

            assert_eq!((raster.width, raster.height), (2, 1));
            assert_eq!((raster.pixels[0] >> 24) as u8, 255);
            assert_eq!((raster.pixels[1] >> 24) as u8, 255);
        }
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
    fn push_svg_scales_linear_gradient_paint_with_svg_transform() {
        let renderer = render_with_options(
            r##"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg"
                    xmlns:xlink="http://www.w3.org/1999/xlink">
                <defs>
                    <rect id="lg1" y2="1" spreadMethod="reflect" width="50" height="50"/>
                </defs>
                <linearGradient id="lg2" xlink:href="#lg1" x2="0.7">
                    <stop offset="0" stop-color="white"/>
                    <stop offset="1" stop-color="black"/>
                </linearGradient>
                <rect x="20" y="20" width="160" height="160" fill="url(#lg2)"/>
            </svg>"##,
            Color::TRANSPARENT,
            SvgOptions {
                transform: Affine::scale(1.5),
                ..Default::default()
            },
            300,
            300,
        );

        let left = renderer.image().rgba8_at(32, 150);
        let middle = renderer.image().rgba8_at(150, 150);
        let end = renderer.image().rgba8_at(210, 150);
        assert!(
            left[0] > 245 && left[3] == 255,
            "left side should stay near white after scene scaling: {left:?}"
        );
        assert!(
            (40..120).contains(&middle[0]) && middle[3] == 255,
            "middle should still be inside the gradient ramp: {middle:?}"
        );
        assert!(
            end[0] < 5 && end[3] == 255,
            "after x2 should clamp to black, not reflect from the rect href: {end:?}"
        );
        assert_eq!(renderer.image().rgba8_at(270, 150)[3], 0);
    }

    #[test]
    fn push_svg_applies_path_transform_to_linear_gradient_paint_server() {
        let renderer = render_with_options(
            r##"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
                <linearGradient id="g" gradientTransform="rotate(30)">
                    <stop offset="0" stop-color="white"/>
                    <stop offset="1" stop-color="black"/>
                </linearGradient>
                <rect x="100" y="40" width="110" height="110"
                      fill="url(#g)" transform="skewX(-30)"/>
            </svg>"##,
            Color::TRANSPARENT,
            SvgOptions {
                transform: Affine::scale(1.5),
                ..Default::default()
            },
            300,
            300,
        );

        let upper = renderer.image().rgba8_at(120, 90);
        let lower = renderer.image().rgba8_at(150, 150);
        let right = renderer.image().rgba8_at(210, 90);
        assert!(
            (180..230).contains(&upper[0]) && upper[3] == 255,
            "upper gradient sample should be light after skew+rotate: {upper:?}"
        );
        assert!(
            (40..100).contains(&lower[0]) && lower[3] == 255,
            "lower gradient sample should be dark after skew+rotate: {lower:?}"
        );
        assert!(
            right[0] < 120 && right[3] == 255,
            "right edge should not stay too light when path transform is applied: {right:?}"
        );
    }

    #[test]
    fn push_svg_scales_radial_gradient_paint_with_svg_transform() {
        let renderer = render_with_options(
            r##"<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
                <defs>
                    <radialGradient id="g" gradientUnits="userSpaceOnUse" cx="50" cy="50" r="40">
                        <stop offset="0" stop-color="white"/>
                        <stop offset="1" stop-color="black"/>
                    </radialGradient>
                </defs>
                <rect x="10" y="10" width="80" height="80" fill="url(#g)"/>
            </svg>"##,
            Color::TRANSPARENT,
            SvgOptions {
                transform: Affine::scale(2.0),
                ..Default::default()
            },
            200,
            200,
        );

        let center = renderer.image().rgba8_at(100, 100);
        let edge = renderer.image().rgba8_at(178, 100);
        assert!(
            center[0] > 245 && center[3] == 255,
            "scaled radial gradient center should stay white: {center:?}"
        );
        assert!(
            edge[0] < 20 && edge[3] == 255,
            "scaled radial gradient edge should be near black: {edge:?}"
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
    fn push_svg_renders_anisotropic_fe_gaussian_blur() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="3" height="3">
                <defs>
                    <filter id="blur" x="0" y="0" width="3" height="3" filterUnits="userSpaceOnUse">
                        <feGaussianBlur stdDeviation="1 0"/>
                    </filter>
                </defs>
                <rect x="1" y="1" width="1" height="1" fill="#ffffff" filter="url(#blur)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert!(renderer.image().rgba8_at(0, 1)[3] > 0);
        assert!(renderer.image().rgba8_at(2, 1)[3] > 0);
        assert_eq!(renderer.image().rgba8_at(1, 0), [0, 0, 0, 0]);
        assert_eq!(renderer.image().rgba8_at(1, 2), [0, 0, 0, 0]);
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
    fn push_svg_fe_offset_preserves_source_outside_viewport_under_transform() {
        let renderer = render_with_options(
            r##"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
                <filter id="filter1">
                    <feOffset dx="20" dy="40"/>
                </filter>
                <rect x="20" y="20" width="100" height="100" fill="seagreen"
                      filter="url(#filter1)" transform="skewX(30) translate(-50)"/>
            </svg>"##,
            Color::TRANSPARENT,
            SvgOptions {
                transform: Affine::scale(1.5),
                ..Default::default()
            },
            300,
            300,
        );

        assert_eq!(renderer.image().rgba8_at(10, 100), [0, 0, 0, 0]);
        assert_eq!(renderer.image().rgba8_at(20, 100), [46, 139, 87, 255]);
        assert_eq!(renderer.image().rgba8_at(220, 190), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_renders_fe_tile_from_unshifted_offset_source_region() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12">
                <defs>
                    <filter id="tile" x="0" y="0" width="24" height="12" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#00ff00" x="1" y="1" width="4" height="4"/>
                        <feOffset dx="2" dy="1"/>
                        <feTile x="0" y="0" width="12" height="8"/>
                    </filter>
                </defs>
                <rect width="12" height="8" fill="#ff0000" filter="url(#tile)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_eq!(renderer.image().rgba8_at(1, 1), [0, 0, 0, 0]);
        assert_eq!(renderer.image().rgba8_at(3, 2), [0, 255, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(7, 6), [0, 255, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(13, 7), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_fe_tile_with_empty_source_region_is_transparent() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="8">
                <defs>
                    <filter id="tile" x="2" y="2" width="8" height="4" filterUnits="userSpaceOnUse">
                        <feFlood flood-color="#ff0000" x="20" y="20" width="2" height="2"/>
                        <feOffset dx="1" dy="1"/>
                        <feTile/>
                    </filter>
                </defs>
                <rect x="2" y="2" width="8" height="4" fill="#00ff00" filter="url(#tile)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert!(renderer.image().pixels.iter().all(|px| *px == 0));
    }

    #[test]
    fn push_svg_scales_fe_offset_with_svg_transform() {
        let renderer = render_with_options(
            r##"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
                <filter id="filter1" filterUnits="userSpaceOnUse" x="0" y="0" width="200" height="200">
                    <feOffset dx="100"/>
                </filter>
                <rect x="20" y="70" width="60" height="60" fill="green"/>
                <rect x="20" y="70" width="60" height="60" fill="red" filter="url(#filter1)"/>
            </svg>"##,
            Color::TRANSPARENT,
            SvgOptions {
                transform: Affine::scale(1.5),
                ..Default::default()
            },
            300,
            300,
        );

        assert_eq!(renderer.image().rgba8_at(90, 150), [0, 128, 0, 255]);
        assert_eq!(renderer.image().rgba8_at(150, 150), [0, 0, 0, 0]);
        assert_eq!(renderer.image().rgba8_at(180, 150), [255, 0, 0, 255]);
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
    fn push_svg_scales_filter_graph_primitive_regions() {
        let renderer = render_with_options(
            r##"<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg">
                <filter id="filter1" color-interpolation-filters="sRGB">
                    <feFlood flood-color="blue"/>
                    <feComposite operator="arithmetic" in2="SourceGraphic"
                        k1="0.1" k2="0.2" k3="0.3" k4="0.4"/>
                </filter>
                <rect x="20" y="20" width="160" height="160" fill="seagreen" filter="url(#filter1)"/>
            </svg>"##,
            Color::TRANSPARENT,
            SvgOptions {
                transform: Affine::scale(1.5),
                ..Default::default()
            },
            300,
            300,
        );

        let inside = renderer.image().rgba8_at(260, 260);
        let flood_only = renderer.image().rgba8_at(10, 10);
        assert!(
            (110..122).contains(&inside[0])
                && (138..150).contains(&inside[1])
                && (182..194).contains(&inside[2])
                && inside[3] == 255,
            "scaled primitive region should not clip the filtered rect: {inside:?}"
        );
        assert_eq!(
            flood_only,
            [102, 102, 153, 153],
            "arithmetic must be evaluated on premultiplied channels before PNG unpremultiply"
        );
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
    fn push_svg_renders_fe_turbulence_in_primitive_region() {
        let renderer = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="noise" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feTurbulence x="4" y="4" width="8" height="8" baseFrequency="0.2" seed="3"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#ff0000" filter="url(#noise)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        let mut covered = 0;
        for y in 4..12 {
            for x in 4..12 {
                covered += usize::from(renderer.image().rgba8_at(x, y)[3] > 0);
            }
        }
        assert!(covered > 0);
        assert_eq!(renderer.image().rgba8_at(2, 2), [0, 0, 0, 0]);
        assert_eq!(renderer.image().rgba8_at(13, 13), [0, 0, 0, 0]);
    }

    #[test]
    fn push_svg_fe_turbulence_respects_color_interpolation_filters() {
        let default_linear = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="noise" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16">
                        <feTurbulence baseFrequency="0.18" seed="4"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#ff0000" filter="url(#noise)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );
        let explicit_srgb = render(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="noise" filterUnits="userSpaceOnUse" x="0" y="0" width="16" height="16"
                            color-interpolation-filters="sRGB">
                        <feTurbulence baseFrequency="0.18" seed="4"/>
                    </filter>
                </defs>
                <rect width="16" height="16" fill="#ff0000" filter="url(#noise)"/>
            </svg>"##,
            Color::TRANSPARENT,
        );

        assert_ne!(
            default_linear.image().rgba8_at(8, 8),
            explicit_srgb.image().rgba8_at(8, 8)
        );
        assert_eq!(
            default_linear.image().rgba8_at(8, 8)[3],
            explicit_srgb.image().rgba8_at(8, 8)[3]
        );
    }

    #[test]
    fn push_svg_unsupported_features_do_not_modify_scene() {
        let tree = parse(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs>
                    <filter id="unsupported">
                        <feDisplacementMap scale="2"/>
                    </filter>
                </defs>
                <g filter="url(#unsupported)"><rect width="16" height="16" fill="#ff0000"/></g>
            </svg>"##,
        );
        let mut scene = Scene::new(16, 16);
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Color::from_rgb8(0, 0, 255),
            FillRule::NonZero,
        );

        let err = scene.push_svg(&tree).unwrap_err();
        assert_eq!(err.feature(), "feDisplacementMap");

        let mut renderer = CpuRenderer::new(16, 16, Color::TRANSPARENT);
        renderer.render(&scene);
        assert_eq!(renderer.image().rgba8_at(8, 8), [0, 0, 255, 255]);
    }
}
