//! SVG fills, stroke outlines, gradients and deferred pattern scenes.
use super::{
    MAX_PATTERN_DEPTH, SvgBuilder, SvgError, SvgOptions,
    geometry::{
        affine_to_array, fill_rule, has_zero_length_subpath, inverse_affine,
        inverse_affine_to_array, resolution_scale, stroke_to_kurbo, tiny_path_to_bez,
        transform_to_affine,
    },
};
use crate::{
    Brush, Canvas, FillRule,
    shared::{
        brush::{PatternBrush, PatternSampling},
        image_resource::ImageResourceId,
    },
};
use peniko::{
    Color, ColorStop, Extend, Gradient, InterpolationAlphaSpace,
    kurbo::{Affine, BezPath},
};
use usvg::{Node, Paint, PaintOrder, SpreadMethod};

impl SvgBuilder {
    pub(super) fn push_path(&self, canvas: &mut Canvas, path: &usvg::Path) -> Result<(), SvgError> {
        if !path.is_visible() {
            return Ok(());
        }

        let data = tiny_path_to_bez(path.data());
        let path_transform = transform_to_affine(path.abs_transform());
        let transform = self.base_transform * path_transform;
        match path.paint_order() {
            PaintOrder::FillAndStroke => {
                self.push_fill(canvas, path, &data, path_transform, transform)?;
                self.push_stroke(canvas, path, &data, path_transform, transform)?;
            }
            PaintOrder::StrokeAndFill => {
                self.push_stroke(canvas, path, &data, path_transform, transform)?;
                self.push_fill(canvas, path, &data, path_transform, transform)?;
            }
        }
        Ok(())
    }

    fn push_fill(
        &self,
        canvas: &mut Canvas,
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

        let brush =
            self.paint_to_brush(canvas, fill.paint(), fill.opacity().get(), path_transform)?;
        let data = self
            .options
            .pixel_grid
            .map_or_else(|| data.clone(), |grid| grid.fill_path(data, transform));
        canvas.push_path(
            data,
            brush,
            transform,
            fill_rule(fill.rule()),
            self.options.tolerance,
        );
        Ok(())
    }

    fn push_stroke(
        &self,
        canvas: &mut Canvas,
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

        let brush = self.paint_to_brush(
            canvas,
            stroke.paint(),
            stroke.opacity().get(),
            path_transform,
        )?;
        let needs_svg_stroker = stroke.linejoin() == usvg::LineJoin::MiterClip
            || (stroke.linecap() != usvg::LineCap::Butt
                && (stroke
                    .dasharray()
                    .is_some_and(|dashes| dashes.contains(&0.0))
                    || has_zero_length_subpath(path.data())));
        if needs_svg_stroker {
            // Kurbo omits zero-length dashes, even though SVG round and square caps must paint
            // them and fully degenerate subpaths. It also lacks miter-clip joins. Dash before
            // expanding the SVG stroke outline: tiny-skia's Path::stroke itself ignores dashes.
            let style = stroke.to_tiny_skia();
            let scale = resolution_scale(transform);
            let dashed = style
                .dash
                .as_ref()
                .map(|dash| path.data().dash(dash, scale));
            let outline = match dashed {
                Some(Some(dashed)) => dashed.stroke(&style, scale),
                Some(None) => None,
                None => path.data().stroke(&style, scale),
            };
            if let Some(outline) = outline {
                canvas.push_path(
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
        let (data, stroke_style) = self.options.pixel_grid.map_or_else(
            || (data.clone(), stroke_style.clone()),
            |grid| grid.stroke_path(data, &stroke_style, transform),
        );
        canvas.push_stroke(
            data,
            stroke_style,
            brush,
            transform,
            FillRule::NonZero,
            self.options.tolerance,
        );
        Ok(())
    }

    fn paint_to_brush(
        &self,
        canvas: &mut Canvas,
        paint: &Paint,
        opacity: f32,
        path_transform: Affine,
    ) -> Result<Brush, SvgError> {
        // SVG gradients interpolate stop colors independently of stop opacity.
        match paint {
            Paint::Color(color) => Ok(color_opacity_to_brush(*color, opacity)),
            Paint::LinearGradient(source) => {
                let stops = gradient_stops(source.stops(), opacity);
                let gradient = Gradient::new_linear(
                    (f64::from(source.x1()), f64::from(source.y1())),
                    (f64::from(source.x2()), f64::from(source.y2())),
                )
                .with_extend(spread_method(source.spread_method()))
                .with_interpolation_alpha_space(InterpolationAlphaSpace::Unpremultiplied)
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
                    (f64::from(source.fx()), f64::from(source.fy())),
                    source.fr().get(),
                    (f64::from(source.cx()), f64::from(source.cy())),
                    source.r().get(),
                )
                .with_extend(spread_method(source.spread_method()))
                .with_interpolation_alpha_space(InterpolationAlphaSpace::Unpremultiplied)
                .with_stops(stops.as_slice());
                let mut brush = Brush::from_gradient(&gradient);
                let Brush::Radial(radial) = &mut brush else {
                    unreachable!();
                };
                radial.transform =
                    self.paint_server_inverse_transform(source.transform(), path_transform)?;
                Ok(brush)
            }
            Paint::Pattern(pattern) => {
                self.pattern_to_brush(canvas, pattern, opacity, path_transform)
            }
        }
    }

    fn paint_server_inverse_transform(
        &self,
        transform: usvg::Transform,
        path_transform: Affine,
    ) -> Result<[f32; 6], SvgError> {
        // usvg lowers paint-server units into the path's user space. Geometry is transformed
        // into canvas coordinates before brush sampling, so fold both path and canvas transforms
        // into the inverse.
        inverse_affine_to_array(
            self.base_transform * path_transform * transform_to_affine(transform),
            "gradientTransform",
        )
    }

    fn pattern_to_brush(
        &self,
        canvas: &mut Canvas,
        pattern: &usvg::Pattern,
        opacity: f32,
        path_transform: Affine,
    ) -> Result<Brush, SvgError> {
        if self.pattern_depth >= MAX_PATTERN_DEPTH {
            return Err(SvgError::unsupported("recursive pattern paint"));
        }

        let rect = pattern.rect();
        let width = rect.width();
        let height = rect.height();
        let tile_width = width.ceil().max(1.0) as u32;
        let tile_height = height.ceil().max(1.0) as u32;
        // Pattern content is local to the tile. The pattern's x/y offset moves
        // tile sampling, not the content drawn into the tile.
        let tile_scale = Affine::scale_non_uniform(
            f64::from(tile_width) / f64::from(width),
            f64::from(tile_height) / f64::from(height),
        );

        let mut tile_scene = Canvas::new(tile_width, tile_height, 1.0);
        Self {
            options: SvgOptions {
                pixel_grid: None,
                ..self.options
            },
            base_transform: tile_scale * pattern_content_correction(pattern.root()),
            pattern_depth: self.pattern_depth + 1,
            image_depth: self.image_depth,
        }
        .push_group(&mut tile_scene, pattern.root())?;

        // The pattern geometry is in the path's user space, while shaders sample
        // the brush in canvas pixels. Account for both SVG and path transforms.
        let world_to_pattern = inverse_affine(
            self.base_transform * path_transform * transform_to_affine(pattern.transform()),
            "patternTransform",
        )?;
        let Some(image_key) = canvas.register_scene_image(tile_scene) else {
            return Err(SvgError::unsupported("empty pattern image"));
        };
        let pattern = PatternBrush::new_resource(
            ImageResourceId::scene(image_key),
            affine_to_array(
                Affine::scale_non_uniform(
                    1.0 / f64::from(tile_width),
                    1.0 / f64::from(tile_height),
                ) * tile_scale
                    * Affine::translate((-f64::from(rect.left()), -f64::from(rect.top())))
                    * world_to_pattern,
            ),
            Extend::Repeat,
            PatternSampling::Nearest,
            opacity_to_u8(opacity),
        );
        Ok(Brush::Pattern(pattern))
    }
}

fn pattern_content_correction(root: &usvg::Group) -> Affine {
    let [Node::Group(wrapper)] = root.children() else {
        return Affine::IDENTITY;
    };
    let transform = transform_to_affine(wrapper.transform());
    if transform == Affine::IDENTITY {
        return transform;
    }

    // usvg may add a viewBox wrapper after resolving objectBoundingBox units.
    // Its descendant absolute transforms then still refer to the old root.
    let descendants_are_unrebased = wrapper.children().iter().any(|child| match child {
        Node::Path(path) => path.abs_transform() == usvg::Transform::default(),
        Node::Group(group) => group.abs_transform() == group.transform(),
        _ => false,
    });
    if descendants_are_unrebased {
        transform
    } else {
        Affine::IDENTITY
    }
}

pub(super) const fn color_opacity_to_brush(color: usvg::Color, opacity: f32) -> Brush {
    Brush::Solid(Color::from_rgb8(color.red, color.green, color.blue).multiply_alpha(opacity))
}

pub(super) fn color_to_rgb(color: usvg::Color) -> [f32; 3] {
    [
        f32::from(color.red) / 255.0,
        f32::from(color.green) / 255.0,
        f32::from(color.blue) / 255.0,
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

const fn spread_method(method: SpreadMethod) -> Extend {
    match method {
        SpreadMethod::Pad => Extend::Pad,
        SpreadMethod::Reflect => Extend::Reflect,
        SpreadMethod::Repeat => Extend::Repeat,
    }
}
