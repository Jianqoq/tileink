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
        brush::{PatternBrush, PatternSampling},
        image::Image as RasterImage,
        layer::filter::{
            COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE, ColorChannel,
            ComponentTransferTable, CompositeOperator, ConvolveEdgeMode, ConvolveMatrix,
            DiffuseLighting, DisplacementMap, FilterInput, FilterPrimitive, FilterPrimitiveKind,
            LightSource, MorphologyOperator, SpecularLighting, Turbulence, TurbulenceKind,
        },
        layer::mask::{Mask as LayerMask, MaskKind},
    },
};

use self::image::{decode_encoded_image, decode_png_image, image_sampling, svg_image_raster_size};

mod image;

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
        self.append(&svg_scene, (0.0, 0.0));
        Ok(())
    }
}

struct SvgBuilder {
    options: SvgOptions,
    base_transform: Affine,
    pattern_depth: u8,
    image_depth: u8,
}

enum ClipPathLowering {
    Empty,
    Fused { path: BezPath, rule: FillRule },
    Mask,
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
                    Region::rect(nonzero_rect_to_kurbo(mask.rect()), Radius::ZERO),
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
        if stroke.linejoin() == usvg::LineJoin::MiterClip {
            // kurbo does not expose SVG 2 miter-clip joins. Build the SVG stroke outline with
            // tiny-skia/usvg semantics, then render that outline through the normal path pipeline.
            if let Some(outline) = path
                .data()
                .stroke(&stroke.to_tiny_skia(), resolution_scale(transform))
            {
                scene.push_path(
                    tiny_path_to_bez(&outline),
                    brush,
                    transform,
                    FillRule::NonZero,
                    self.options.tolerance,
                );
            }
            return Ok(());
        }

        let stroke_style = stroke_to_kurbo(stroke);
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

        match self.clip_path_lowering(clip) {
            ClipPathLowering::Empty => scene.push_mask_layer(
                Scene::new(scene.width, scene.height),
                LayerMask {
                    region: Region::rect(Rect::ZERO, Radius::ZERO),
                    kind: MaskKind::Alpha,
                },
            ),
            ClipPathLowering::Fused { path, rule } => scene.push_clip_layer(
                path,
                self.base_transform * transform_to_affine(clip.transform()),
                rule,
                self.options.tolerance,
            ),
            ClipPathLowering::Mask => {
                let (mask_scene, mask) =
                    self.svg_clip_path_mask_layer(scene.width, scene.height, clip)?;
                scene.push_mask_layer(mask_scene, mask);
            }
        }
        Ok(pushed + 1)
    }

    fn svg_clip_path_mask_layer(
        &self,
        width: u32,
        height: u32,
        clip: &usvg::ClipPath,
    ) -> Result<(Scene, LayerMask), SvgError> {
        let mut mask_scene = Scene::new(width, height);
        self.push_clip_path_mask_group(
            &mut mask_scene,
            clip.root(),
            self.base_transform * transform_to_affine(clip.transform()),
        )?;
        Ok((
            mask_scene,
            LayerMask {
                // Complex clipPath lowering must not crop group contents before filters run.
                // The alpha mask applies the actual clip shape after isolated content rendering.
                region: Region::rect(
                    Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
                    Radius::ZERO,
                ),
                kind: MaskKind::Alpha,
            },
        ))
    }

    fn push_clip_path_mask_group(
        &self,
        scene: &mut Scene,
        group: &usvg::Group,
        transform: Affine,
    ) -> Result<(), SvgError> {
        let transform = transform * transform_to_affine(group.transform());
        let mut pushed_layers = 0;
        if let Some(clip) = group.clip_path() {
            pushed_layers += SvgBuilder {
                options: self.options,
                base_transform: transform,
                pattern_depth: self.pattern_depth,
                image_depth: self.image_depth,
            }
            .push_clip_path_layers(scene, clip)?;
        }

        for child in group.children() {
            self.push_clip_path_mask_node(scene, child, transform)?;
        }

        for _ in 0..pushed_layers {
            scene.pop_layer();
        }
        Ok(())
    }

    fn push_clip_path_mask_node(
        &self,
        scene: &mut Scene,
        node: &Node,
        transform: Affine,
    ) -> Result<(), SvgError> {
        match node {
            Node::Group(group) => self.push_clip_path_mask_group(scene, group, transform),
            Node::Path(path) => {
                if path.is_visible()
                    && let Some(fill) = path.fill()
                {
                    scene.push_path(
                        tiny_path_to_bez(path.data()),
                        Brush::Solid(Color::BLACK),
                        transform,
                        fill_rule(fill.rule()),
                        self.options.tolerance,
                    );
                }
                Ok(())
            }
            Node::Text(text) => self.push_clip_path_mask_group(scene, text.flattened(), transform),
            Node::Image(_) => Ok(()),
        }
    }

    fn clip_path_lowering(&self, clip: &usvg::ClipPath) -> ClipPathLowering {
        let mut paths = Vec::new();
        let mut needs_mask = false;
        self.collect_clip_path_candidates(clip.root(), &mut paths, &mut needs_mask);
        if paths.is_empty() && !needs_mask {
            ClipPathLowering::Empty
        } else if !needs_mask && paths.len() == 1 {
            let (path, rule) = paths.pop().unwrap();
            ClipPathLowering::Fused { path, rule }
        } else {
            ClipPathLowering::Mask
        }
    }

    fn collect_clip_path_candidates(
        &self,
        group: &usvg::Group,
        paths: &mut Vec<(BezPath, FillRule)>,
        needs_mask: &mut bool,
    ) {
        if group.clip_path().is_some() {
            *needs_mask = true;
            return;
        }

        for child in group.children() {
            match child {
                Node::Group(group) => self.collect_clip_path_candidates(group, paths, needs_mask),
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
                Node::Text(text) => {
                    self.collect_clip_path_candidates(text.flattened(), paths, needs_mask)
                }
                Node::Image(_) => {}
            }
        }
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
    let mut graph =
        SvgFilterGraphBuilder::new(builder, region_transform, content_transform, filter_bounds);
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
            region: transform_region(Region::rect(filter_rect, Radius::ZERO), region_transform),
        }))
    }
}

struct SvgFilterGraphBuilder<'a> {
    builder: &'a SvgBuilder,
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
        region_transform: Affine,
        content_transform: Affine,
        filter_bounds: Bounds,
    ) -> Self {
        Self {
            builder,
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
        &self,
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
                let kind = filter_to_primitive_kind(color_matrix_to_filter(matrix.kind())?);
                (self.input(matrix.input(), "feColorMatrix")?, None, kind)
            }
            usvg::filter::Kind::ComponentTransfer(transfer) => {
                let kind = filter_to_primitive_kind(component_transfer_to_filter(transfer)?);
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
                FilterPrimitiveKind::Filter(Box::new(diffuse_lighting_to_filter(lighting))),
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
                FilterPrimitiveKind::Filter(Box::new(specular_lighting_to_filter(lighting))),
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
        let source_region = self.output_source_region(primitive, input, input2, &kind, region);
        Ok(LoweredSvgFilterPrimitive {
            primitive: FilterPrimitive {
                input,
                input2,
                region,
                kind,
            },
            source_region,
        })
    }

    fn output_source_region(
        &self,
        primitive: &usvg::filter::Primitive,
        input: FilterInput,
        input2: Option<FilterInput>,
        kind: &FilterPrimitiveKind,
        region: Bounds,
    ) -> Bounds {
        let region = region.intersect(self.filter_bounds);
        match primitive.kind() {
            usvg::filter::Kind::Flood(_)
            | usvg::filter::Kind::Image(_)
            | usvg::filter::Kind::Turbulence(_) => region,
            usvg::filter::Kind::Offset(_) => self.input_source_region(input).intersect(region),
            usvg::filter::Kind::GaussianBlur(blur) => {
                let (std_dev_x, std_dev_y) = transform_filter_radii(
                    self.value_transform,
                    blur.std_dev_x().get(),
                    blur.std_dev_y().get(),
                );
                self.input_source_region(input)
                    .outset(blur_outset(std_dev_x.max(std_dev_y)))
                    .intersect(region)
            }
            usvg::filter::Kind::Morphology(morphology) => {
                let radius = match morphology.operator() {
                    usvg::filter::MorphologyOperator::Dilate => {
                        transform_filter_radius_x(self.value_transform, morphology.radius_x().get())
                            .max(transform_filter_radius_y(
                                self.value_transform,
                                morphology.radius_y().get(),
                            ))
                            .max(0.0)
                            .ceil() as i32
                    }
                    usvg::filter::MorphologyOperator::Erode => 0,
                };
                self.input_source_region(input)
                    .outset(radius)
                    .intersect(region)
            }
            usvg::filter::Kind::Blend(_) => self
                .input_source_region(input)
                .union(self.input_source_region(
                    input2.expect("feBlend lowering produced no second input"),
                ))
                .intersect(region),
            usvg::filter::Kind::Composite(composite) => {
                let input_region = self.input_source_region(input);
                let input2_region = self.input_source_region(
                    input2.expect("feComposite lowering produced no second input"),
                );
                match composite.operator() {
                    usvg::filter::CompositeOperator::In => input_region.intersect(input2_region),
                    usvg::filter::CompositeOperator::Out => input_region,
                    _ => input_region.union(input2_region),
                }
                .intersect(region)
            }
            usvg::filter::Kind::DisplacementMap(displacement) => {
                let (scale_x, scale_y) = transform_filter_radii(
                    self.value_transform,
                    displacement.scale(),
                    displacement.scale(),
                );
                let source_outset = (scale_x.abs().max(scale_y.abs()) * 0.5).ceil() as i32;
                self.input_source_region(input)
                    .outset(source_outset)
                    .intersect(region)
            }
            usvg::filter::Kind::Merge(_) => match kind {
                FilterPrimitiveKind::Merge { inputs } => inputs
                    .iter()
                    .map(|input| self.input_source_region(*input))
                    .fold(Bounds::new(0, 0, 0, 0), BoundsExt::union)
                    .intersect(region),
                _ => region,
            },
            usvg::filter::Kind::Tile(_) => match kind {
                FilterPrimitiveKind::Tile { source_region } if !source_region.is_empty() => region,
                _ => Bounds::new(0, 0, 0, 0),
            },
            _ => self.input_source_region(input).intersect(region),
        }
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

fn blur_outset(std_dev: f32) -> i32 {
    (std_dev.max(0.0) * 3.0).ceil() as i32
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

fn displacement_map_to_filter(
    displacement: &usvg::filter::DisplacementMap,
    color_interpolation: usvg::filter::ColorInterpolation,
    transform: Affine,
) -> DisplacementMap {
    let (scale_x, scale_y) =
        transform_filter_radii(transform, displacement.scale(), displacement.scale());
    DisplacementMap {
        scale_x,
        scale_y,
        x_channel: color_channel(displacement.x_channel_selector()),
        y_channel: color_channel(displacement.y_channel_selector()),
        linear_rgb: color_interpolation == usvg::filter::ColorInterpolation::LinearRGB,
    }
}

fn color_channel(channel: usvg::filter::ColorChannel) -> ColorChannel {
    match channel {
        usvg::filter::ColorChannel::R => ColorChannel::R,
        usvg::filter::ColorChannel::G => ColorChannel::G,
        usvg::filter::ColorChannel::B => ColorChannel::B,
        usvg::filter::ColorChannel::A => ColorChannel::A,
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

fn resolution_scale(transform: Affine) -> f32 {
    let [xx, yx, xy, yy, _, _] = transform.as_coeffs();
    let scale = xx.hypot(yx).max(xy.hypot(yy));
    if scale.is_finite() && scale > 0.0 {
        scale as f32
    } else {
        1.0
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

fn stroke_to_kurbo(stroke: &usvg::Stroke) -> Stroke {
    let mut out = Stroke::new(stroke.width().get() as f64)
        .with_join(match stroke.linejoin() {
            usvg::LineJoin::Miter => Join::Miter,
            usvg::LineJoin::Round => Join::Round,
            usvg::LineJoin::Bevel => Join::Bevel,
            usvg::LineJoin::MiterClip => unreachable!("handled by tiny-skia stroke outline"),
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
    out
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
mod tests;
