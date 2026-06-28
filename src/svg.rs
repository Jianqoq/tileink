use std::{error::Error, fmt};

use peniko::{
    Color, ColorStop, Compose, Extend, Gradient, Mix,
    kurbo::{Affine, BezPath, Cap, Join, Rect, Shape, Stroke},
};
use usvg::{Node, Paint, PaintOrder, SpreadMethod, tiny_skia_path::PathSegment};

use crate::{Brush, FillRule, Scene};

#[derive(Clone, Copy, Debug)]
pub struct SvgOptions {
    pub tolerance: f64,
}

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
        SvgBuilder { options }.push_tree(&mut svg_scene, tree)?;
        self.merge(svg_scene);
        Ok(())
    }
}

struct SvgBuilder {
    options: SvgOptions,
}

impl SvgBuilder {
    fn push_tree(&self, scene: &mut Scene, tree: &usvg::Tree) -> Result<(), SvgError> {
        self.push_group(scene, tree.root())
    }

    fn push_group(&self, scene: &mut Scene, group: &usvg::Group) -> Result<(), SvgError> {
        if group.mask().is_some() {
            return Err(SvgError::unsupported("mask"));
        }
        if !group.filters().is_empty() {
            return Err(SvgError::unsupported("filter"));
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

        let mut pushed_layers = 0;
        if let Some(clip) = group.clip_path() {
            pushed_layers += self.push_clip_path_layers(scene, clip)?;
        }

        let layer_path = || rect_path(nonzero_rect_to_kurbo(group.abs_layer_bounding_box()));
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
        let transform = transform_to_affine(path.abs_transform());
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

        let brush = paint_to_brush(fill.paint(), fill.opacity().get())?;
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

        let brush = paint_to_brush(stroke.paint(), stroke.opacity().get())?;
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
            transform_to_affine(clip.transform()),
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

fn paint_to_brush(paint: &Paint, opacity: f32) -> Result<Brush, SvgError> {
    match paint {
        Paint::Color(color) => Ok(Brush::Solid(
            Color::from_rgb8(color.red, color.green, color.blue).multiply_alpha(opacity),
        )),
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
            linear.transform = inverse_transform_array(transform)?;
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
            radial.transform = inverse_transform_array(transform)?;
            Ok(brush)
        }
        Paint::Pattern(_) => Err(SvgError::unsupported("pattern paint")),
    }
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

fn inverse_transform_array(transform: usvg::Transform) -> Result<[f32; 6], SvgError> {
    let Some(transform) = transform.invert() else {
        return Err(SvgError::unsupported("non-invertible gradientTransform"));
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
        let mut scene = Scene::new(size.width().ceil() as u32, size.height().ceil() as u32);
        scene.push_svg(&tree).unwrap();
        let mut renderer = CpuRenderer::new(scene.width, scene.height, clear);
        renderer.render(&scene);
        renderer
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
    fn push_svg_unsupported_features_do_not_modify_scene() {
        let tree = parse(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                <defs><filter id="blur"><feGaussianBlur stdDeviation="2"/></filter></defs>
                <g filter="url(#blur)"><rect width="16" height="16" fill="#ff0000"/></g>
            </svg>"##,
        );
        let mut scene = Scene::new(16, 16);
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Color::from_rgb8(0, 0, 255),
            FillRule::NonZero,
        );

        let err = scene.push_svg(&tree).unwrap_err();
        assert_eq!(err.feature(), "filter");

        let mut renderer = CpuRenderer::new(16, 16, Color::TRANSPARENT);
        renderer.render(&scene);
        assert_eq!(renderer.image().rgba8_at(8, 8), [0, 0, 255, 255]);
    }
}
