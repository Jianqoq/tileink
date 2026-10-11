//! Primitive parameter conversion after usvg has resolved SVG units.
use super::super::{geometry::transform_filter_radii, paint::color_to_rgb};
use crate::{
    Filter,
    shared::{
        bounds::Bounds,
        layer::filter::{
            ColorChannel, CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting,
            DisplacementMap, LightSource, MorphologyOperator, SpecularLighting, Turbulence,
            TurbulenceKind,
        },
    },
};
use peniko::kurbo::Affine;

pub(super) fn composite_operator(operator: usvg::filter::CompositeOperator) -> CompositeOperator {
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

pub(super) fn convolve_matrix_to_filter(convolve: &usvg::filter::ConvolveMatrix) -> Filter {
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

pub(super) fn diffuse_lighting_to_filter(
    lighting: &usvg::filter::DiffuseLighting,
    transform: Affine,
) -> Filter {
    Filter::DiffuseLighting(DiffuseLighting {
        surface_scale: lighting.surface_scale(),
        diffuse_constant: lighting.diffuse_constant(),
        lighting_color: color_to_rgb(lighting.lighting_color()),
        light_source: light_source(lighting.light_source(), transform),
    })
}

pub(super) fn specular_lighting_to_filter(
    lighting: &usvg::filter::SpecularLighting,
    transform: Affine,
) -> Filter {
    Filter::SpecularLighting(SpecularLighting {
        surface_scale: lighting.surface_scale(),
        specular_constant: lighting.specular_constant(),
        specular_exponent: lighting.specular_exponent(),
        lighting_color: color_to_rgb(lighting.lighting_color()),
        light_source: light_source(lighting.light_source(), transform),
    })
}

pub(super) fn turbulence_to_filter(
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

pub(super) fn displacement_map_to_filter(
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

fn light_source(source: usvg::filter::LightSource, transform: Affine) -> LightSource {
    // SVG light positions use primitive coordinates, while the lighting kernel uses canvas pixels.
    let point = |x: f32, y: f32| {
        let point = transform * peniko::kurbo::Point::new(x as f64, y as f64);
        (point.x as f32, point.y as f32)
    };
    let [a, b, c, d, _, _] = transform.as_coeffs();
    // Extend the 2D transform to the height axis without letting rotation or skew change depth.
    let z_scale = (a * d - b * c).abs().sqrt() as f32;
    match source {
        usvg::filter::LightSource::DistantLight(light) => LightSource::Distant {
            azimuth: light.azimuth,
            elevation: light.elevation,
        },
        usvg::filter::LightSource::PointLight(light) => {
            let (x, y) = point(light.x, light.y);
            LightSource::Point {
                x,
                y,
                z: light.z * z_scale,
            }
        }
        usvg::filter::LightSource::SpotLight(light) => {
            let (x, y) = point(light.x, light.y);
            let (points_at_x, points_at_y) = point(light.points_at_x, light.points_at_y);
            LightSource::Spot {
                x,
                y,
                z: light.z * z_scale,
                points_at_x,
                points_at_y,
                points_at_z: light.points_at_z * z_scale,
                specular_exponent: light.specular_exponent.get(),
                limiting_cone_angle: light.limiting_cone_angle,
            }
        }
    }
}

pub(super) fn morphology_operator(
    operator: usvg::filter::MorphologyOperator,
) -> MorphologyOperator {
    match operator {
        usvg::filter::MorphologyOperator::Erode => MorphologyOperator::Erode,
        usvg::filter::MorphologyOperator::Dilate => MorphologyOperator::Dilate,
    }
}
