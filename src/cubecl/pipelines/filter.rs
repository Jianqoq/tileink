use ::cubecl::prelude::*;

use crate::{
    cubecl::{
        brush::GpuBrushResources,
        buffer::CubeBuffer,
        pipelines::common::{
            blend_premul_u8, combine_alpha, pack_premul_rgba8, sample_brush, scale_premul_u8,
            src_over_premul_u8,
        },
        profile::profile_launch,
        renderer::{ScanBuffers, SceneBuffers},
        types::{
            CUBE_DRAW_BLEND, CUBE_DRAW_BRUSH, CUBE_DRAW_CLIP, CUBE_DRAW_ISOLATE, CUBE_DRAW_OPACITY,
            CUBE_DRAW_PATH_GLYPH, CUBE_LAYER_BLEND, CUBE_LAYER_CLIP, CUBE_LAYER_OPACITY,
            CUBE_SDF_ARC, CUBE_SDF_ARC_SHADOW, CUBE_SDF_CANDLESTICK, CUBE_SDF_CIRCLE,
            CUBE_SDF_CIRCLE_SHADOW, CUBE_SDF_CIRCLE_STROKE, CUBE_SDF_LINE, CUBE_SDF_LINE_SHADOW,
            CUBE_SDF_NONE, CUBE_SDF_RECT, CUBE_SDF_RECT_SHADOW, CUBE_SDF_RECT_STROKE,
        },
    },
    shared::{
        bounds::Bounds,
        layer::filter::{
            LIQUID_GLASS_ACTIVE_DISTANCE_NORM, LIQUID_GLASS_CHROMATIC_B, LIQUID_GLASS_CHROMATIC_G,
            LIQUID_GLASS_CHROMATIC_R, LIQUID_GLASS_D65_X, LIQUID_GLASS_D65_Y, LIQUID_GLASS_D65_Z,
            LIQUID_GLASS_EDGE_BLEND_END, LIQUID_GLASS_EDGE_BLEND_START, LIQUID_GLASS_EPSILON,
            LIQUID_GLASS_FRESNEL_LIGHTNESS_GAIN, LIQUID_GLASS_FRESNEL_MIX_SCALE,
            LIQUID_GLASS_GEOMETRY_DISTANCE_SCALE, LIQUID_GLASS_GEOMETRY_RANGE_SCALE,
            LIQUID_GLASS_GLARE_CHROMA_GAIN, LIQUID_GLASS_GLARE_LIGHTNESS_GAIN,
            LIQUID_GLASS_GLARE_POWER_BASE, LIQUID_GLASS_GLARE_POWER_SCALE,
            LIQUID_GLASS_GLARE_SIDE_SCALE, LIQUID_GLASS_NORMAL_LENGTH_SCALE, LIQUID_GLASS_PI,
            LIQUID_GLASS_REFRACTION_PIXEL_SCALE, LIQUID_GLASS_TINT_BASE_MIX, LIQUID_GLASS_TINT_MIX,
            RectLiquidGlass, RectLiquidGlassRegion,
        },
    },
};

const FILTER_WORKGROUP_SIZE: u32 = 256;
const COMPONENT_TRANSFER_TABLE_SIZE_U32: u32 =
    crate::shared::layer::filter::COMPONENT_TRANSFER_TABLE_SIZE as u32;
const COMPONENT_TRANSFER_TABLE_LEN_U32: u32 =
    crate::shared::layer::filter::COMPONENT_TRANSFER_TABLE_LEN as u32;
const TURBULENCE_TABLE_LEN_U32: u32 = crate::shared::layer::filter::TURBULENCE_TABLE_LEN as u32;
const TURBULENCE_GRADIENT_LEN_U32: u32 =
    crate::shared::layer::filter::TURBULENCE_GRADIENT_LEN as u32;

pub(crate) const FILTER_BRIGHTNESS: u32 = 1;
pub(crate) const FILTER_CONTRAST: u32 = 2;
pub(crate) const FILTER_GRAYSCALE: u32 = 3;
pub(crate) const FILTER_HUE_ROTATE: u32 = 4;
pub(crate) const FILTER_INVERT: u32 = 5;
pub(crate) const FILTER_OPACITY: u32 = 6;
pub(crate) const FILTER_SATURATE: u32 = 7;
pub(crate) const FILTER_SEPIA: u32 = 8;
pub(crate) const SVG_MASK_ALPHA: u32 = 0;
pub(crate) const SVG_MASK_LUMINANCE: u32 = 1;

pub(crate) struct FilterPathResources<'a> {
    pub(crate) range_starts: &'a CubeBuffer<u32>,
    pub(crate) range_ends: &'a CubeBuffer<u32>,
    pub(crate) p0x: &'a CubeBuffer<i32>,
    pub(crate) p0y: &'a CubeBuffer<i32>,
    pub(crate) p1x: &'a CubeBuffer<i32>,
    pub(crate) p1y: &'a CubeBuffer<i32>,
}

pub(crate) struct FilterPipeline;

impl FilterPipeline {
    pub(crate) fn clear_region<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_clear_region", || {
            kernels::filter_clear_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn copy_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_copy_region", || {
            kernels::filter_copy_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn tile_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        source_bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        let Some(source_region) = FilterRegion::new(size, source_bounds) else {
            return;
        };
        profile_launch(client, "filter_tile_region", || {
            kernels::filter_tile_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                source_region.x0,
                source_region.y0,
                source_region.width,
                source_region.height,
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn source_alpha_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_source_alpha_region", || {
            kernels::filter_source_alpha_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn svg_mask_coverage_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        kind: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_svg_mask_coverage_region", || {
            kernels::filter_svg_mask_coverage_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                kind,
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn apply_region_mask<R: Runtime>(
        client: &ComputeClient<R>,
        mask: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_apply_region_mask", || {
            kernels::filter_apply_region_mask::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                unsafe { mask.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn source_over_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_source_over_region", || {
            kernels::filter_source_over_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn blend_region<R: Runtime>(
        client: &ComputeClient<R>,
        input1: &CubeBuffer<u32>,
        input2: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        mode: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_blend_region", || {
            kernels::filter_blend_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                mode,
                unsafe { input1.arg() },
                unsafe { input2.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_inputs_region<R: Runtime>(
        client: &ComputeClient<R>,
        input1: &CubeBuffer<u32>,
        input2: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        operator: u32,
        arithmetic: [f32; 4],
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_composite_inputs_region", || {
            kernels::filter_composite_inputs_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                operator,
                arithmetic[0],
                arithmetic[1],
                arithmetic[2],
                arithmetic[3],
                unsafe { input1.arg() },
                unsafe { input2.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn displacement_map_region<R: Runtime>(
        client: &ComputeClient<R>,
        input1: &CubeBuffer<u32>,
        input2: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        scale_x: f32,
        scale_y: f32,
        x_channel: u32,
        y_channel: u32,
        linear_rgb: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_displacement_map_region", || {
            kernels::filter_displacement_map_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                size.1,
                scale_x,
                scale_y,
                x_channel,
                y_channel,
                linear_rgb,
                unsafe { input1.arg() },
                unsafe { input2.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn morphology_axis_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        radius: u32,
        operator: u32,
        axis: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_morphology_axis_region", || {
            kernels::filter_morphology_axis_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                size.1,
                radius,
                operator,
                axis,
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn apply_color_filter<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        filter_kind: u32,
        amount: f32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_color_region", || {
            kernels::filter_color_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                filter_kind,
                amount,
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_color_matrix<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        matrix: [f32; 20],
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_color_matrix_region", || {
            kernels::filter_color_matrix_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                matrix[0],
                matrix[1],
                matrix[2],
                matrix[3],
                matrix[4],
                matrix[5],
                matrix[6],
                matrix[7],
                matrix[8],
                matrix[9],
                matrix[10],
                matrix[11],
                matrix[12],
                matrix[13],
                matrix[14],
                matrix[15],
                matrix[16],
                matrix[17],
                matrix[18],
                matrix[19],
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn apply_component_transfer<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        table_index: u32,
        transfer_tables: &CubeBuffer<u32>,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_component_transfer_region", || {
            kernels::filter_component_transfer_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                table_index,
                unsafe { transfer_tables.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn turbulence_region<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        base_frequency_x: f32,
        base_frequency_y: f32,
        num_octaves: u32,
        stitch_tiles: u32,
        kind: u32,
        linear_rgb: u32,
        table_index: u32,
        transform_x: f32,
        transform_y: f32,
        scale_x: f32,
        scale_y: f32,
        tile_x: f32,
        tile_y: f32,
        tile_width: f32,
        tile_height: f32,
        selectors: &CubeBuffer<u32>,
        gradients: &CubeBuffer<f32>,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_turbulence_region", || {
            kernels::filter_turbulence_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                base_frequency_x,
                base_frequency_y,
                num_octaves,
                stitch_tiles,
                kind,
                linear_rgb,
                table_index,
                transform_x,
                transform_y,
                scale_x,
                scale_y,
                tile_x,
                tile_y,
                tile_width,
                tile_height,
                unsafe { selectors.arg() },
                unsafe { gradients.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rect_liquid_glass_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        blurred: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        glass: RectLiquidGlass,
        region: RectLiquidGlassRegion,
    ) {
        let Some(dispatch) = FilterRegion::new(size, bounds) else {
            return;
        };
        // liquid-glass-studio treats tint as straight RGBA. Premultiplying here
        // turns transparent white into black and breaks Fresnel/glare tinting.
        let [tint_r, tint_g, tint_b, tint_a] = glass.tint.components;
        profile_launch(client, "filter_liquid_glass_region", || {
            kernels::filter_liquid_glass_region::launch::<R>(
                client,
                cube_count(dispatch.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                dispatch.pixel_count,
                dispatch.width,
                dispatch.x0,
                dispatch.y0,
                size.0,
                size.1,
                region.x0,
                region.y0,
                region.x1,
                region.y1,
                region.radius_top_left,
                region.radius_top_right,
                region.radius_bottom_left,
                region.radius_bottom_right,
                u32::from(glass.blur_edge),
                tint_r,
                tint_g,
                tint_b,
                tint_a,
                glass.refraction_thickness,
                glass.refraction_factor,
                glass.refraction_dispersion,
                glass.fresnel_range,
                glass.fresnel_hardness * 0.01,
                glass.fresnel_factor * 0.01,
                glass.glare_range,
                glass.glare_hardness * 0.01,
                glass.glare_convergence * 0.01,
                glass.glare_opposite_factor * 0.01,
                glass.glare_factor * 0.01,
                glass.glare_angle,
                unsafe { source.arg() },
                unsafe { blurred.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn convolve_matrix_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        kernels: &CubeBuffer<f32>,
        kernel_offset: u32,
        columns: u32,
        rows: u32,
        target_x: u32,
        target_y: u32,
        divisor: f32,
        bias: f32,
        edge_mode: u32,
        preserve_alpha: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_convolve_matrix_region", || {
            kernels::filter_convolve_matrix_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.height,
                region.x0,
                region.y0,
                size.0,
                kernel_offset,
                columns,
                rows,
                target_x,
                target_y,
                divisor,
                bias,
                edge_mode,
                preserve_alpha,
                unsafe { kernels.arg() },
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lighting_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        output_kind: u32,
        surface_scale: f32,
        light_constant: f32,
        specular_exponent: f32,
        lighting_color: [f32; 3],
        surface_origin: (i32, i32),
        light_kind: u32,
        light_params: [f32; 9],
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_lighting_region", || {
            kernels::filter_lighting_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.height,
                region.x0,
                region.y0,
                size.0,
                output_kind,
                surface_scale,
                light_constant,
                specular_exponent,
                lighting_color[0],
                lighting_color[1],
                lighting_color[2],
                surface_origin.0,
                surface_origin.1,
                light_kind,
                light_params[0],
                light_params[1],
                light_params[2],
                light_params[3],
                light_params[4],
                light_params[5],
                light_params[6],
                light_params[7],
                light_params[8],
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn offset_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_offset_region", || {
            kernels::filter_offset_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.height,
                region.x0,
                region.y0,
                size.0,
                dx,
                dy,
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn flood_region<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        brush_index: u32,
        brushes: GpuBrushResources<'_>,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_flood_region", || {
            kernels::filter_flood_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                brush_index,
                unsafe { brushes.data.arg() },
                unsafe { brushes.params.arg() },
                unsafe { brushes.payloads.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn composite_src_over_region<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        source: &CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_composite_region", || {
            kernels::filter_composite_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_src_over_stack_region<R: Runtime>(
        client: &ComputeClient<R>,
        scene: &SceneBuffers,
        scan: &ScanBuffers,
        target: &mut CubeBuffer<u32>,
        source: &CubeBuffer<u32>,
        mask: Option<&CubeBuffer<u32>>,
        size: (u32, u32),
        bounds: Bounds,
        layer_stack_start: u32,
        layer_stack_end: u32,
        group_stack_capacity: usize,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        let mask_enabled = u32::from(mask.is_some());
        let mask = mask.unwrap_or(source);
        profile_launch(client, "filter_composite_stack_region", || {
            kernels::filter_composite_stack_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                FILTER_WORKGROUP_SIZE as usize,
                group_stack_capacity.max(1),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                size.0.div_ceil(16),
                size.1.div_ceil(16),
                layer_stack_start,
                layer_stack_end,
                mask_enabled,
                unsafe { source.arg() },
                unsafe { mask.arg() },
                unsafe { scene.draw_path_ids.arg() },
                unsafe { scene.draw_tags.arg() },
                unsafe { scene.draw_fill_rules.arg() },
                unsafe { scene.draw_pixel_x0.arg() },
                unsafe { scene.draw_pixel_y0.arg() },
                unsafe { scene.draw_pixel_x1.arg() },
                unsafe { scene.draw_pixel_y1.arg() },
                unsafe { scene.draw_sdf_kinds.arg() },
                unsafe { scene.draw_sdf_x0.arg() },
                unsafe { scene.draw_sdf_y0.arg() },
                unsafe { scene.draw_sdf_x1.arg() },
                unsafe { scene.draw_sdf_y1.arg() },
                unsafe { scene.draw_sdf_r0.arg() },
                unsafe { scene.draw_sdf_r1.arg() },
                unsafe { scene.draw_sdf_r2.arg() },
                unsafe { scene.draw_sdf_r3.arg() },
                unsafe { scene.draw_sdf_stroke_top.arg() },
                unsafe { scene.draw_sdf_stroke_right.arg() },
                unsafe { scene.draw_sdf_stroke_bottom.arg() },
                unsafe { scene.draw_sdf_stroke_left.arg() },
                unsafe { scene.draw_sdf_shadow_offset_x.arg() },
                unsafe { scene.draw_sdf_shadow_offset_y.arg() },
                unsafe { scene.draw_sdf_shadow_expand.arg() },
                unsafe { scene.draw_sdf_shadow_intensity.arg() },
                unsafe { scene.backdrop_data_offsets.arg() },
                unsafe { scene.backdrop_tile_x0.arg() },
                unsafe { scene.backdrop_tile_y0.arg() },
                unsafe { scene.backdrop_tile_x1.arg() },
                unsafe { scene.backdrop_tile_y1.arg() },
                unsafe { scan.backdrops.arg() },
                unsafe { scan.tile_segment_range_starts.arg() },
                unsafe { scan.tile_segment_range_ends.arg() },
                unsafe { scan.segment_p0x.arg() },
                unsafe { scan.segment_p0y.arg() },
                unsafe { scan.segment_p1x.arg() },
                unsafe { scan.segment_p1y.arg() },
                unsafe { scan.segment_y_edge.arg() },
                unsafe { scene.plan_layer_stack_tags.arg() },
                unsafe { scene.plan_layer_stack_draws.arg() },
                unsafe { scene.plan_layer_stack_payloads.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_src_over_surface_stack_region<R: Runtime>(
        client: &ComputeClient<R>,
        scene: &SceneBuffers,
        scan: &ScanBuffers,
        target: &mut CubeBuffer<u32>,
        source: &CubeBuffer<u32>,
        target_size: (u32, u32),
        source_size: (u32, u32),
        source_origin: (i32, i32),
        bounds: Bounds,
        layer_stack_start: u32,
        layer_stack_end: u32,
        group_stack_capacity: usize,
    ) {
        let Some(region) = FilterRegion::new(target_size, bounds) else {
            return;
        };
        profile_launch(client, "filter_composite_surface_stack_region", || {
            kernels::filter_composite_surface_stack_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                FILTER_WORKGROUP_SIZE as usize,
                group_stack_capacity.max(1),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                target_size.0,
                target_size.0.div_ceil(16),
                target_size.1.div_ceil(16),
                source_size.0,
                source_size.1,
                source_origin.0,
                source_origin.1,
                layer_stack_start,
                layer_stack_end,
                unsafe { source.arg() },
                unsafe { scene.draw_path_ids.arg() },
                unsafe { scene.draw_tags.arg() },
                unsafe { scene.draw_fill_rules.arg() },
                unsafe { scene.draw_pixel_x0.arg() },
                unsafe { scene.draw_pixel_y0.arg() },
                unsafe { scene.draw_pixel_x1.arg() },
                unsafe { scene.draw_pixel_y1.arg() },
                unsafe { scene.draw_sdf_kinds.arg() },
                unsafe { scene.draw_sdf_x0.arg() },
                unsafe { scene.draw_sdf_y0.arg() },
                unsafe { scene.draw_sdf_x1.arg() },
                unsafe { scene.draw_sdf_y1.arg() },
                unsafe { scene.draw_sdf_r0.arg() },
                unsafe { scene.draw_sdf_r1.arg() },
                unsafe { scene.draw_sdf_r2.arg() },
                unsafe { scene.draw_sdf_r3.arg() },
                unsafe { scene.draw_sdf_stroke_top.arg() },
                unsafe { scene.draw_sdf_stroke_right.arg() },
                unsafe { scene.draw_sdf_stroke_bottom.arg() },
                unsafe { scene.draw_sdf_stroke_left.arg() },
                unsafe { scene.draw_sdf_shadow_offset_x.arg() },
                unsafe { scene.draw_sdf_shadow_offset_y.arg() },
                unsafe { scene.draw_sdf_shadow_expand.arg() },
                unsafe { scene.draw_sdf_shadow_intensity.arg() },
                unsafe { scene.backdrop_data_offsets.arg() },
                unsafe { scene.backdrop_tile_x0.arg() },
                unsafe { scene.backdrop_tile_y0.arg() },
                unsafe { scene.backdrop_tile_x1.arg() },
                unsafe { scene.backdrop_tile_y1.arg() },
                unsafe { scan.backdrops.arg() },
                unsafe { scan.tile_segment_range_starts.arg() },
                unsafe { scan.tile_segment_range_ends.arg() },
                unsafe { scan.segment_p0x.arg() },
                unsafe { scan.segment_p0y.arg() },
                unsafe { scan.segment_p1x.arg() },
                unsafe { scan.segment_p1y.arg() },
                unsafe { scan.segment_y_edge.arg() },
                unsafe { scene.plan_layer_stack_tags.arg() },
                unsafe { scene.plan_layer_stack_draws.arg() },
                unsafe { scene.plan_layer_stack_payloads.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_blend_stack_region<R: Runtime>(
        client: &ComputeClient<R>,
        scene: &SceneBuffers,
        scan: &ScanBuffers,
        target: &mut CubeBuffer<u32>,
        source: &CubeBuffer<u32>,
        mask: &CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        layer_stack_start: u32,
        layer_stack_end: u32,
        group_stack_capacity: usize,
        mode: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_composite_blend_stack_region", || {
            kernels::filter_composite_blend_stack_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                FILTER_WORKGROUP_SIZE as usize,
                group_stack_capacity.max(1),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                size.0.div_ceil(16),
                size.1.div_ceil(16),
                layer_stack_start,
                layer_stack_end,
                mode,
                unsafe { source.arg() },
                unsafe { mask.arg() },
                unsafe { scene.draw_path_ids.arg() },
                unsafe { scene.draw_tags.arg() },
                unsafe { scene.draw_fill_rules.arg() },
                unsafe { scene.draw_pixel_x0.arg() },
                unsafe { scene.draw_pixel_y0.arg() },
                unsafe { scene.draw_pixel_x1.arg() },
                unsafe { scene.draw_pixel_y1.arg() },
                unsafe { scene.draw_sdf_kinds.arg() },
                unsafe { scene.draw_sdf_x0.arg() },
                unsafe { scene.draw_sdf_y0.arg() },
                unsafe { scene.draw_sdf_x1.arg() },
                unsafe { scene.draw_sdf_y1.arg() },
                unsafe { scene.draw_sdf_r0.arg() },
                unsafe { scene.draw_sdf_r1.arg() },
                unsafe { scene.draw_sdf_r2.arg() },
                unsafe { scene.draw_sdf_r3.arg() },
                unsafe { scene.draw_sdf_stroke_top.arg() },
                unsafe { scene.draw_sdf_stroke_right.arg() },
                unsafe { scene.draw_sdf_stroke_bottom.arg() },
                unsafe { scene.draw_sdf_stroke_left.arg() },
                unsafe { scene.draw_sdf_shadow_offset_x.arg() },
                unsafe { scene.draw_sdf_shadow_offset_y.arg() },
                unsafe { scene.draw_sdf_shadow_expand.arg() },
                unsafe { scene.draw_sdf_shadow_intensity.arg() },
                unsafe { scene.backdrop_data_offsets.arg() },
                unsafe { scene.backdrop_tile_x0.arg() },
                unsafe { scene.backdrop_tile_y0.arg() },
                unsafe { scene.backdrop_tile_x1.arg() },
                unsafe { scene.backdrop_tile_y1.arg() },
                unsafe { scan.backdrops.arg() },
                unsafe { scan.tile_segment_range_starts.arg() },
                unsafe { scan.tile_segment_range_ends.arg() },
                unsafe { scan.segment_p0x.arg() },
                unsafe { scan.segment_p0y.arg() },
                unsafe { scan.segment_p1x.arg() },
                unsafe { scan.segment_p1y.arg() },
                unsafe { scan.segment_y_edge.arg() },
                unsafe { scene.plan_layer_stack_tags.arg() },
                unsafe { scene.plan_layer_stack_draws.arg() },
                unsafe { scene.plan_layer_stack_payloads.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rasterize_layer_mask<R: Runtime>(
        client: &ComputeClient<R>,
        scene: &SceneBuffers,
        scan: &ScanBuffers,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        draw: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_layer_mask_region", || {
            kernels::filter_layer_mask_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                size.0.div_ceil(16),
                size.1.div_ceil(16),
                draw,
                unsafe { scene.draw_path_ids.arg() },
                unsafe { scene.draw_tags.arg() },
                unsafe { scene.draw_fill_rules.arg() },
                unsafe { scene.draw_pixel_x0.arg() },
                unsafe { scene.draw_pixel_y0.arg() },
                unsafe { scene.draw_pixel_x1.arg() },
                unsafe { scene.draw_pixel_y1.arg() },
                unsafe { scene.draw_sdf_kinds.arg() },
                unsafe { scene.draw_sdf_x0.arg() },
                unsafe { scene.draw_sdf_y0.arg() },
                unsafe { scene.draw_sdf_x1.arg() },
                unsafe { scene.draw_sdf_y1.arg() },
                unsafe { scene.draw_sdf_r0.arg() },
                unsafe { scene.draw_sdf_r1.arg() },
                unsafe { scene.draw_sdf_r2.arg() },
                unsafe { scene.draw_sdf_r3.arg() },
                unsafe { scene.draw_sdf_stroke_top.arg() },
                unsafe { scene.draw_sdf_stroke_right.arg() },
                unsafe { scene.draw_sdf_stroke_bottom.arg() },
                unsafe { scene.draw_sdf_stroke_left.arg() },
                unsafe { scene.draw_sdf_shadow_offset_x.arg() },
                unsafe { scene.draw_sdf_shadow_offset_y.arg() },
                unsafe { scene.draw_sdf_shadow_expand.arg() },
                unsafe { scene.draw_sdf_shadow_intensity.arg() },
                unsafe { scene.backdrop_data_offsets.arg() },
                unsafe { scene.backdrop_tile_x0.arg() },
                unsafe { scene.backdrop_tile_y0.arg() },
                unsafe { scene.backdrop_tile_x1.arg() },
                unsafe { scene.backdrop_tile_y1.arg() },
                unsafe { scan.backdrops.arg() },
                unsafe { scan.tile_segment_range_starts.arg() },
                unsafe { scan.tile_segment_range_ends.arg() },
                unsafe { scan.segment_p0x.arg() },
                unsafe { scan.segment_p0y.arg() },
                unsafe { scan.segment_p1x.arg() },
                unsafe { scan.segment_p1y.arg() },
                unsafe { scan.segment_y_edge.arg() },
                unsafe { target.arg() },
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rasterize_rect_mask<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        rect: (f32, f32, f32, f32),
        radius: (f32, f32, f32, f32),
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_rect_mask_region", || {
            kernels::filter_rect_mask_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                rect.0,
                rect.1,
                rect.2,
                rect.3,
                radius.0,
                radius.1,
                radius.2,
                radius.3,
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn rasterize_path_mask<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        path_index: u32,
        paths: FilterPathResources<'_>,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_path_mask_region", || {
            kernels::filter_path_mask_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                path_index,
                unsafe { paths.range_starts.arg() },
                unsafe { paths.range_ends.arg() },
                unsafe { paths.p0x.arg() },
                unsafe { paths.p0y.arg() },
                unsafe { paths.p1x.arg() },
                unsafe { paths.p1y.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn blur_pass<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        std_dev: f32,
        axis: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_blur_region", || {
            kernels::filter_blur_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.height,
                region.x0,
                region.y0,
                size.0,
                std_dev,
                axis,
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn build_drop_shadow_mask<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_drop_shadow_mask_region", || {
            kernels::filter_drop_shadow_mask_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.height,
                region.x0,
                region.y0,
                size.0,
                dx,
                dy,
                unsafe { source.arg() },
                unsafe { target.arg() },
            );
        });
    }

    pub(crate) fn composite_drop_shadow<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        shadow_mask: &CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        brush_index: u32,
        brushes: GpuBrushResources<'_>,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        profile_launch(client, "filter_composite_drop_shadow_region", || {
            kernels::filter_composite_drop_shadow_region::launch::<R>(
                client,
                cube_count(region.pixel_count),
                CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
                region.pixel_count,
                region.width,
                region.x0,
                region.y0,
                size.0,
                brush_index,
                unsafe { brushes.data.arg() },
                unsafe { brushes.params.arg() },
                unsafe { brushes.payloads.arg() },
                unsafe { shadow_mask.arg() },
                unsafe { target.arg() },
            );
        });
    }
}

#[derive(Clone, Copy)]
struct FilterRegion {
    x0: u32,
    y0: u32,
    width: u32,
    height: u32,
    pixel_count: u32,
}

impl FilterRegion {
    fn new(size: (u32, u32), bounds: Bounds) -> Option<Self> {
        let canvas = Bounds::canvas(size.0, size.1);
        let bounds = bounds.intersect(canvas);
        if bounds.is_empty() {
            return None;
        }
        let width = bounds.width();
        let height = bounds.height();
        Some(Self {
            x0: bounds.x0 as u32,
            y0: bounds.y0 as u32,
            width,
            height,
            pixel_count: width * height,
        })
    }
}

fn cube_count(items: u32) -> CubeCount {
    CubeCount::Static(items.div_ceil(FILTER_WORKGROUP_SIZE), 1, 1)
}

mod kernels;
