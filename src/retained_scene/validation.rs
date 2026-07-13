use super::model::*;
use super::prelude::*;

pub(super) fn validate_size(width: u32, height: u32, scale: f32) -> Result<(), RetainedSceneError> {
    (width > 0 && height > 0 && scale.is_finite() && scale > 0.0)
        .then_some(())
        .ok_or(RetainedSceneError::InvalidSize)
}

pub(super) fn validate_transform(transform: Affine) -> Result<(), RetainedSceneError> {
    let coefficients = transform.as_coeffs();
    (coefficients.iter().all(|value| value.is_finite())
        && (coefficients[0] * coefficients[3] - coefficients[1] * coefficients[2]).abs()
            > f64::EPSILON)
        .then_some(())
        .ok_or(RetainedSceneError::InvalidTransform)
}

pub(super) fn affine_linear_part_eq(left: Affine, right: Affine) -> bool {
    left.as_coeffs()[..4] == right.as_coeffs()[..4]
}

pub(super) fn validate_damage_rect(rect: Rect) -> Result<(), RetainedSceneError> {
    (rect.x0.is_finite()
        && rect.y0.is_finite()
        && rect.x1.is_finite()
        && rect.y1.is_finite()
        && rect.width() > 0.0
        && rect.height() > 0.0)
        .then_some(())
        .ok_or(RetainedSceneError::InvalidSize)
}

pub(super) fn validate_canvas(canvas: &Canvas, scale: f32) -> Result<(), RetainedSceneError> {
    if !canvas.is_closed_for_append() {
        return Err(RetainedSceneError::UnclosedCanvas);
    }
    ((canvas.scale_factor() - scale).abs() <= f32::EPSILON)
        .then_some(())
        .ok_or(RetainedSceneError::ScaleMismatch)
}

pub(super) fn validate_kind(kind: &NodeKind, scale: f32) -> Result<(), RetainedSceneError> {
    match kind {
        NodeKind::Scene {
            canvas,
            transform,
            translation_damage,
        } => {
            validate_canvas(canvas, scale)?;
            validate_transform(*transform)?;
            if let Some(damage) = translation_damage {
                validate_damage_rect(*damage)?;
            }
            Ok(())
        }
        NodeKind::Layer(layer) => validate_layer(layer),
        NodeKind::Group => Ok(()),
    }
}

pub(super) fn validate_layer(layer: &RetainedLayerDescriptor) -> Result<(), RetainedSceneError> {
    match layer {
        RetainedLayerDescriptor::ClipPath {
            path,
            transform,
            tolerance,
            ..
        }
        | RetainedLayerDescriptor::Isolate {
            path,
            transform,
            tolerance,
        }
        | RetainedLayerDescriptor::Opacity {
            path,
            transform,
            tolerance,
            ..
        }
        | RetainedLayerDescriptor::Blend {
            path,
            transform,
            tolerance,
            ..
        } => {
            validate_path(path)?;
            if !transform.as_coeffs().into_iter().all(f64::is_finite)
                || !tolerance.is_finite()
                || *tolerance < 0.0
            {
                return Err(RetainedSceneError::InvalidPosition);
            }
            if let RetainedLayerDescriptor::Opacity { opacity, .. } = layer
                && (!opacity.is_finite() || !(0.0..=1.0).contains(opacity))
            {
                return Err(RetainedSceneError::InvalidPosition);
            }
            Ok(())
        }
        RetainedLayerDescriptor::Filter { sample_region, .. }
        | RetainedLayerDescriptor::Backdrop { sample_region, .. } => validate_region(sample_region),
        RetainedLayerDescriptor::Mask(mask) => validate_region(&mask.region),
        RetainedLayerDescriptor::ClipSdf { sdf, transform } => {
            validate_sdf(*sdf)?;
            validate_transform(*transform)
        }
    }
}

pub(super) fn validate_sdf(sdf: Sdf) -> Result<(), RetainedSceneError> {
    let point = |point: Point| point.x.is_finite() && point.y.is_finite();
    let floats = |values: &[f32]| values.iter().all(|value| value.is_finite());
    let radius = |radius: crate::Radius| {
        floats(&[
            radius.top_left,
            radius.top_right,
            radius.bottom_left,
            radius.bottom_right,
        ])
    };
    let rect = |rect: crate::shared::sdf::rect::Rect| {
        point(rect.start) && point(rect.end) && radius(rect.radius)
    };
    let circle = |circle: crate::shared::sdf::circle::Circle| {
        point(circle.center) && circle.radius.is_finite()
    };
    let line = |line: crate::shared::sdf::line::Line| {
        point(line.start) && point(line.end) && line.width.is_finite()
    };
    let valid = match sdf {
        Sdf::Rect(value) => rect(value),
        Sdf::RectStroke(value) => {
            rect(value.rect)
                && floats(&[
                    value.widths.top,
                    value.widths.right,
                    value.widths.bottom,
                    value.widths.left,
                ])
        }
        Sdf::Circle(value) => circle(value),
        Sdf::CircleStroke(value) => circle(value.circle) && value.half_width.is_finite(),
        Sdf::Rc(value) => {
            point(value.center)
                && floats(&[
                    value.radius,
                    value.start_angle,
                    value.sweep_angle,
                    value.width,
                ])
        }
        Sdf::CandleStick(value) => floats(&[
            value.center_x,
            value.high_y,
            value.low_y,
            value.body_top_y,
            value.body_bottom_y,
        ]),
        Sdf::Line(value) => line(value),
        Sdf::DashLine(value) => {
            line(value.line) && floats(&[value.dash_length, value.gap_length, value.dash_offset])
        }
    };
    valid
        .then_some(())
        .ok_or(RetainedSceneError::InvalidPosition)
}

pub(super) fn validate_region(region: &Region) -> Result<(), RetainedSceneError> {
    match region {
        Region::Rect { rect, .. } => [rect.x0, rect.y0, rect.x1, rect.y1]
            .into_iter()
            .all(f64::is_finite)
            .then_some(())
            .ok_or(RetainedSceneError::InvalidPosition),
        Region::Path {
            path,
            transform,
            tolerance,
        } => {
            validate_path(path)?;
            (transform.as_coeffs().into_iter().all(f64::is_finite)
                && tolerance.is_finite()
                && *tolerance >= 0.0)
                .then_some(())
                .ok_or(RetainedSceneError::InvalidPosition)
        }
    }
}

pub(super) fn validate_path(path: &BezPath) -> Result<(), RetainedSceneError> {
    let finite = |point: Point| point.x.is_finite() && point.y.is_finite();
    path.elements()
        .iter()
        .all(|element| match *element {
            PathEl::MoveTo(point) | PathEl::LineTo(point) => finite(point),
            PathEl::QuadTo(a, b) => finite(a) && finite(b),
            PathEl::CurveTo(a, b, c) => finite(a) && finite(b) && finite(c),
            PathEl::ClosePath => true,
        })
        .then_some(())
        .ok_or(RetainedSceneError::InvalidPosition)
}
