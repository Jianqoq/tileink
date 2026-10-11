//! Geometry conversion shared by SVG drawing and filter lowering.
use super::SvgError;
use crate::{FillRule, shared::bounds::Bounds};
use peniko::{
    Mix,
    kurbo::{Affine, BezPath, Cap, Join, Rect, Shape, Stroke},
};
use usvg::tiny_skia_path::PathSegment;

pub(super) fn resolution_scale(transform: Affine) -> f32 {
    let [xx, yx, xy, yy, _, _] = transform.as_coeffs();
    let scale = xx.hypot(yx).max(xy.hypot(yy));
    if scale.is_finite() && scale > 0.0 {
        scale as f32
    } else {
        1.0
    }
}

pub(super) fn tiny_path_to_bez(path: &usvg::tiny_skia_path::Path) -> BezPath {
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

pub(super) fn has_zero_length_subpath(path: &usvg::tiny_skia_path::Path) -> bool {
    let mut start = None;
    let mut has_draw_command = false;
    let mut stays_at_start = true;
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(point) => {
                if has_draw_command && stays_at_start {
                    return true;
                }
                start = Some(point);
                has_draw_command = false;
                stays_at_start = true;
            }
            PathSegment::LineTo(point) => {
                has_draw_command = true;
                stays_at_start &= start == Some(point);
            }
            PathSegment::QuadTo(control, point) => {
                has_draw_command = true;
                stays_at_start &= start == Some(control) && start == Some(point);
            }
            PathSegment::CubicTo(first, second, point) => {
                has_draw_command = true;
                stays_at_start &=
                    start == Some(first) && start == Some(second) && start == Some(point);
            }
            PathSegment::Close => has_draw_command = true,
        }
    }
    has_draw_command && stays_at_start
}

pub(super) fn stroke_to_kurbo(stroke: &usvg::Stroke) -> Stroke {
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

pub(super) fn fill_rule(rule: usvg::FillRule) -> FillRule {
    match rule {
        usvg::FillRule::NonZero => FillRule::NonZero,
        usvg::FillRule::EvenOdd => FillRule::EvenOdd,
    }
}

pub(super) fn blend_mode_to_mix(mode: usvg::BlendMode) -> Mix {
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

pub(super) fn transform_to_affine(transform: usvg::Transform) -> Affine {
    Affine::new([
        transform.sx as f64,
        transform.ky as f64,
        transform.kx as f64,
        transform.sy as f64,
        transform.tx as f64,
        transform.ty as f64,
    ])
}

pub(super) fn affine_to_array(transform: Affine) -> [f32; 6] {
    transform.as_coeffs().map(|value| value as f32)
}

pub(super) fn inverse_affine(transform: Affine, feature: &str) -> Result<Affine, SvgError> {
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

pub(super) fn inverse_affine_to_array(
    transform: Affine,
    feature: &str,
) -> Result<[f32; 6], SvgError> {
    inverse_affine(transform, feature).map(affine_to_array)
}

pub(super) fn filter_axis_scale(transform: Affine) -> Affine {
    let [a, b, c, d, _, _] = transform.as_coeffs();
    // Filter primitive values are applied in the axis-aligned filter buffer.
    // Preserve canvas scale, but do not let skew/rotation mix `dx` into `dy`.
    Affine::scale_non_uniform(a.hypot(c), b.hypot(d))
}

pub(super) fn transform_filter_vector(transform: Affine, x: f32, y: f32) -> (f32, f32) {
    // SVG filter lengths are in user space, while renderer filter kernels run in canvas pixels.
    let [a, b, c, d, _, _] = transform.as_coeffs();
    (
        (a * x as f64 + c * y as f64) as f32,
        (b * x as f64 + d * y as f64) as f32,
    )
}

pub(super) fn transform_filter_radii(transform: Affine, x: f32, y: f32) -> (f32, f32) {
    (
        transform_filter_radius_x(transform, x),
        transform_filter_radius_y(transform, y),
    )
}

pub(super) fn transform_filter_radius_x(transform: Affine, radius: f32) -> f32 {
    let (x, y) = transform_filter_vector(transform, radius, 0.0);
    x.hypot(y)
}

pub(super) fn transform_filter_radius_y(transform: Affine, radius: f32) -> f32 {
    let (x, y) = transform_filter_vector(transform, 0.0, radius);
    x.hypot(y)
}

pub(super) fn transform_rect_to_bounds(rect: Rect, transform: Affine) -> Bounds {
    rect_to_bounds(if transform == Affine::IDENTITY {
        rect
    } else {
        transform.transform_rect_bbox(rect)
    })
}

pub(super) fn rect_to_bounds(rect: Rect) -> Bounds {
    Bounds::new(
        rect.x0.floor() as i32,
        rect.y0.floor() as i32,
        rect.x1.ceil() as i32,
        rect.y1.ceil() as i32,
    )
}

pub(super) fn rect_path(rect: Rect) -> BezPath {
    rect.to_path(0.0)
}

pub(super) fn nonzero_rect_to_kurbo(rect: usvg::NonZeroRect) -> Rect {
    Rect::new(
        rect.left() as f64,
        rect.top() as f64,
        rect.right() as f64,
        rect.bottom() as f64,
    )
}
