//! Shader parameter encodings for native frame execution.
//! One conversion path keeps both shader languages on the same filter semantics.

use super::filter_config::FilterConfig;
use super::layer::filter::{
    BlurDownsampleFilter, BlurUpsampleFilter, ColorChannel, CompositeOperator, ConvolveEdgeMode,
    LightSource, RectLiquidGlass, RectLiquidGlassRegion, TurbulenceKind,
};
use super::layer::filter::{ConvolveMatrix, DisplacementMap, Turbulence};

pub(crate) fn encode_convolve_edge_mode(edge_mode: ConvolveEdgeMode) -> u32 {
    match edge_mode {
        ConvolveEdgeMode::None => 0,
        ConvolveEdgeMode::Duplicate => 1,
        ConvolveEdgeMode::Wrap => 2,
    }
}

pub(crate) fn encode_color_channel(channel: ColorChannel) -> u32 {
    match channel {
        ColorChannel::R => 0,
        ColorChannel::G => 1,
        ColorChannel::B => 2,
        ColorChannel::A => 3,
    }
}

pub(crate) fn encode_light_source_kind(light_source: LightSource) -> u32 {
    match light_source {
        LightSource::Distant { .. } => 0,
        LightSource::Point { .. } => 1,
        LightSource::Spot { .. } => 2,
    }
}

pub(crate) fn light_source_params(light_source: LightSource) -> [f32; 9] {
    match light_source {
        LightSource::Distant { azimuth, elevation } => {
            [azimuth, elevation, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0]
        }
        LightSource::Point { x, y, z } => [x, y, z, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0],
        LightSource::Spot {
            x,
            y,
            z,
            points_at_x,
            points_at_y,
            points_at_z,
            specular_exponent,
            limiting_cone_angle,
        } => [
            x,
            y,
            z,
            points_at_x,
            points_at_y,
            points_at_z,
            specular_exponent,
            limiting_cone_angle.unwrap_or(-1.0),
            0.0,
        ],
    }
}

pub(crate) fn encode_composite_operator(operator: CompositeOperator) -> u32 {
    match operator {
        CompositeOperator::Over => 0,
        CompositeOperator::In => 1,
        CompositeOperator::Out => 2,
        CompositeOperator::Atop => 3,
        CompositeOperator::Xor => 4,
        CompositeOperator::Arithmetic { .. } => 5,
    }
}

pub(crate) fn composite_arithmetic(operator: CompositeOperator) -> [f32; 4] {
    match operator {
        CompositeOperator::Arithmetic { k1, k2, k3, k4 } => [k1, k2, k3, k4],
        _ => [0.0; 4],
    }
}

pub(crate) fn encode_turbulence_kind(kind: TurbulenceKind) -> u32 {
    match kind {
        TurbulenceKind::Turbulence => 0,
        TurbulenceKind::FractalNoise => 1,
    }
}

pub(crate) fn encode_blur_downsample_filter(filter: BlurDownsampleFilter) -> u32 {
    match filter {
        BlurDownsampleFilter::Nearest => 0,
        BlurDownsampleFilter::Box => 1,
    }
}

pub(crate) fn encode_blur_upsample_filter(filter: BlurUpsampleFilter) -> u32 {
    match filter {
        BlurUpsampleFilter::Nearest => 0,
        BlurUpsampleFilter::Bilinear => 1,
    }
}

pub(crate) fn configure_rect_liquid_glass(
    config: &mut FilterConfig,
    glass: RectLiquidGlass,
    region: RectLiquidGlassRegion,
) {
    let [tint_r, tint_g, tint_b, tint_a] = glass.tint.components;
    config.rect_x0 = region.x0;
    config.rect_y0 = region.y0;
    config.rect_x1 = region.x1;
    config.rect_y1 = region.y1;
    config.radius_top_left = region.radius_top_left;
    config.radius_top_right = region.radius_top_right;
    config.radius_bottom_left = region.radius_bottom_left;
    config.radius_bottom_right = region.radius_bottom_right;
    config.mask_enabled = u32::from(glass.blur_edge);
    config.liquid_tint_r = tint_r;
    config.liquid_tint_g = tint_g;
    config.liquid_tint_b = tint_b;
    config.liquid_tint_a = tint_a;
    config.liquid_refraction_thickness = glass.refraction_thickness;
    config.liquid_refraction_factor = glass.refraction_factor;
    config.liquid_refraction_dispersion = glass.refraction_dispersion;
    config.liquid_fresnel_range = glass.fresnel_range;
    config.liquid_fresnel_hardness = glass.fresnel_hardness * 0.01;
    config.liquid_fresnel_factor = glass.fresnel_factor * 0.01;
    config.liquid_glare_range = glass.glare_range;
    config.liquid_glare_hardness = glass.glare_hardness * 0.01;
    config.liquid_glare_convergence = glass.glare_convergence * 0.01;
    config.liquid_glare_opposite_factor = glass.glare_opposite_factor * 0.01;
    config.liquid_glare_factor = glass.glare_factor * 0.01;
    config.liquid_glare_angle = glass.glare_angle;
}

pub(crate) fn configure_convolve(
    config: &mut FilterConfig,
    matrix: &ConvolveMatrix,
    kernel_offset: u32,
) {
    config.kernel_offset = kernel_offset;
    config.kernel_columns = matrix.columns;
    config.kernel_rows = matrix.rows;
    config.kernel_target_x = matrix.target_x;
    config.kernel_target_y = matrix.target_y;
    config.kernel_edge_mode = encode_convolve_edge_mode(matrix.edge_mode);
    config.kernel_preserve_alpha = u32::from(matrix.preserve_alpha);
    config.amount = matrix.divisor;
    config.rect_x0 = matrix.bias;
}

pub(crate) fn configure_displacement(config: &mut FilterConfig, displacement: &DisplacementMap) {
    config.amount = displacement.scale_x;
    config.rect_x0 = displacement.scale_y;
    config.kernel_edge_mode = encode_color_channel(displacement.x_channel);
    config.kernel_preserve_alpha = encode_color_channel(displacement.y_channel);
    config.lighting_output_kind = u32::from(displacement.linear_rgb);
}

pub(crate) fn configure_turbulence(
    config: &mut FilterConfig,
    turbulence: &Turbulence,
    table_index: u32,
) {
    config.table_index = table_index;
    config.turbulence_base_frequency_x = turbulence.base_frequency_x;
    config.turbulence_base_frequency_y = turbulence.base_frequency_y;
    config.turbulence_num_octaves = turbulence.num_octaves;
    config.turbulence_stitch_tiles = u32::from(turbulence.stitch_tiles);
    config.turbulence_kind = encode_turbulence_kind(turbulence.kind);
    config.linear_rgb = u32::from(turbulence.linear_rgb);
    config.turbulence_transform_x = turbulence.transform_x;
    config.turbulence_transform_y = turbulence.transform_y;
    config.turbulence_scale_x = turbulence.scale_x;
    config.turbulence_scale_y = turbulence.scale_y;
    config.turbulence_tile_x = turbulence.tile_x;
    config.turbulence_tile_y = turbulence.tile_y;
    config.turbulence_tile_width = turbulence.tile_width;
    config.turbulence_tile_height = turbulence.tile_height;
}

pub(crate) fn configure_color_matrix(config: &mut FilterConfig, matrix: [f32; 20]) {
    config.matrix_r = [matrix[0], matrix[1], matrix[2], matrix[3]];
    config.matrix_g = [matrix[5], matrix[6], matrix[7], matrix[8]];
    config.matrix_b = [matrix[10], matrix[11], matrix[12], matrix[13]];
    config.matrix_a = [matrix[15], matrix[16], matrix[17], matrix[18]];
    config.matrix_bias = [matrix[4], matrix[9], matrix[14], matrix[19]];
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn configure_lighting(
    config: &mut FilterConfig,
    output_kind: u32,
    surface_scale: f32,
    light_constant: f32,
    specular_exponent: f32,
    lighting_color: [f32; 3],
    light_source: LightSource,
    surface_origin: (i32, i32),
) {
    let light_params = light_source_params(light_source);
    config.lighting_output_kind = output_kind;
    config.surface_scale = surface_scale;
    config.light_constant = light_constant;
    config.specular_exponent = specular_exponent;
    config.light_r = lighting_color[0];
    config.light_g = lighting_color[1];
    config.light_b = lighting_color[2];
    config.light_kind = encode_light_source_kind(light_source);
    config.light_p0 = light_params[0];
    config.light_p1 = light_params[1];
    config.light_p2 = light_params[2];
    config.light_p3 = light_params[3];
    config.light_p4 = light_params[4];
    config.light_p5 = light_params[5];
    config.light_p6 = light_params[6];
    config.light_p7 = light_params[7];
    config.light_p8 = light_params[8];
    config.surface_origin_x = surface_origin.0;
    config.surface_origin_y = surface_origin.1;
}
