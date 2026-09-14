use super::super::abi::Scalar;
use super::*;
#[path = "../../../src/shared/filter_config.rs"]
mod filter_config;
use filter_config::FilterConfig;

pub(super) fn config() -> Resource {
    macro_rules! field {
        ($name:ident, $scalar:ident, $lanes:expr) => {
            Field {
                name: stringify!($name).into(),
                offset: std::mem::offset_of!(FilterConfig, $name) as u32,
                lanes: $lanes,
                scalar: Scalar::$scalar,
            }
        };
    }
    Resource {
        binding: 0,
        kind: Kind::Uniform,
        size: std::mem::size_of::<FilterConfig>() as u32,
        internal: false,
        fields: vec![
            field!(width, U32, 1),
            field!(height, U32, 1),
            field!(tiles_width, U32, 1),
            field!(tiles_height, U32, 1),
            field!(region_x0, U32, 1),
            field!(region_y0, U32, 1),
            field!(region_width, U32, 1),
            field!(region_height, U32, 1),
            field!(pixel_count, U32, 1),
            field!(active_tile_count, U32, 1),
            field!(compact_tiles, U32, 1),
            field!(dispatch_width, U32, 1),
            field!(active_tile_pad1, U32, 1),
            field!(downsample, U32, 1),
            field!(downsample_filter, U32, 1),
            field!(upsample_filter, U32, 1),
            field!(downsample_pad, U32, 1),
            field!(source_x0, U32, 1),
            field!(source_y0, U32, 1),
            field!(source_x1, U32, 1),
            field!(source_y1, U32, 1),
            field!(layer_stack_start, U32, 1),
            field!(layer_stack_end, U32, 1),
            field!(draw_ix, U32, 1),
            field!(mask_enabled, U32, 1),
            field!(blend_mode, U32, 1),
            field!(mask_kind, U32, 1),
            field!(clear_color, U32, 1),
            field!(filter_kind, U32, 1),
            field!(table_index, U32, 1),
            field!(brush_offset, U32, 1),
            field!(paint_sdf_shadow_base, U32, 1),
            field!(offset_x, I32, 1),
            field!(offset_y, I32, 1),
            field!(morphology_radius, U32, 1),
            field!(morphology_operator, U32, 1),
            field!(morphology_axis, U32, 1),
            field!(blur_axis, U32, 1),
            field!(kernel_offset, U32, 1),
            field!(kernel_columns, U32, 1),
            field!(kernel_rows, U32, 1),
            field!(kernel_target_x, U32, 1),
            field!(kernel_target_y, U32, 1),
            field!(kernel_edge_mode, U32, 1),
            field!(kernel_preserve_alpha, U32, 1),
            field!(lighting_output_kind, U32, 1),
            field!(surface_origin_x, I32, 1),
            field!(surface_origin_y, I32, 1),
            field!(light_kind, U32, 1),
            field!(amount, F32, 1),
            field!(rect_x0, F32, 1),
            field!(rect_y0, F32, 1),
            field!(rect_x1, F32, 1),
            field!(rect_y1, F32, 1),
            field!(radius_top_left, F32, 1),
            field!(radius_top_right, F32, 1),
            field!(radius_bottom_left, F32, 1),
            field!(radius_bottom_right, F32, 1),
            field!(surface_scale, F32, 1),
            field!(light_constant, F32, 1),
            field!(specular_exponent, F32, 1),
            field!(light_r, F32, 1),
            field!(light_g, F32, 1),
            field!(light_b, F32, 1),
            field!(light_p0, F32, 1),
            field!(light_p1, F32, 1),
            field!(light_p2, F32, 1),
            field!(light_p3, F32, 1),
            field!(light_p4, F32, 1),
            field!(light_p5, F32, 1),
            field!(light_p6, F32, 1),
            field!(light_p7, F32, 1),
            field!(light_p8, F32, 1),
            field!(turbulence_base_frequency_x, F32, 1),
            field!(turbulence_base_frequency_y, F32, 1),
            field!(turbulence_num_octaves, U32, 1),
            field!(turbulence_stitch_tiles, U32, 1),
            field!(turbulence_kind, U32, 1),
            field!(turbulence_linear_rgb, U32, 1),
            field!(turbulence_pad0, U32, 1),
            field!(turbulence_pad1, U32, 1),
            field!(turbulence_transform_x, F32, 1),
            field!(turbulence_transform_y, F32, 1),
            field!(turbulence_scale_x, F32, 1),
            field!(turbulence_scale_y, F32, 1),
            field!(turbulence_tile_x, F32, 1),
            field!(turbulence_tile_y, F32, 1),
            field!(turbulence_tile_width, F32, 1),
            field!(turbulence_tile_height, F32, 1),
            field!(liquid_tint_r, F32, 1),
            field!(liquid_tint_g, F32, 1),
            field!(liquid_tint_b, F32, 1),
            field!(liquid_tint_a, F32, 1),
            field!(liquid_refraction_thickness, F32, 1),
            field!(liquid_refraction_factor, F32, 1),
            field!(liquid_refraction_dispersion, F32, 1),
            field!(liquid_fresnel_range, F32, 1),
            field!(liquid_fresnel_hardness, F32, 1),
            field!(liquid_fresnel_factor, F32, 1),
            field!(liquid_glare_range, F32, 1),
            field!(liquid_glare_hardness, F32, 1),
            field!(liquid_glare_convergence, F32, 1),
            field!(liquid_glare_opposite_factor, F32, 1),
            field!(liquid_glare_factor, F32, 1),
            field!(liquid_glare_angle, F32, 1),
            field!(matrix_pad0, U32, 1),
            field!(matrix_pad1, U32, 1),
            field!(matrix_pad2, U32, 1),
            field!(matrix_r, F32, 4),
            field!(matrix_g, F32, 4),
            field!(matrix_b, F32, 4),
            field!(matrix_a, F32, 4),
            field!(matrix_bias, F32, 4),
        ],
    }
}

pub(super) fn basic(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("source_texture", buffer(1, Kind::Texture)),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
        ],
        &[
            (
                "filter_color_region",
                &["config", "target_texture", "active_tiles"],
            ),
            (
                "filter_color_matrix_region",
                &["config", "target_texture", "active_tiles"],
            ),
            (
                "filter_clear_region",
                &["config", "target_texture", "active_tiles"],
            ),
            (
                "filter_copy_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
            (
                "filter_source_over_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
            (
                "filter_svg_mask_coverage_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
            (
                "filter_source_alpha_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
            (
                "filter_tile_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
            (
                "filter_offset_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
            (
                "filter_drop_shadow_mask_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
        ],
    )
}

pub(super) fn inputs(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("source_texture", buffer(1, Kind::Texture)),
            ("aux_texture", buffer(2, Kind::Texture)),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
        ],
        &[
            (
                "filter_blend_region",
                &[
                    "config",
                    "source_texture",
                    "aux_texture",
                    "target_texture",
                    "active_tiles",
                ],
            ),
            (
                "filter_composite_inputs_region",
                &[
                    "config",
                    "source_texture",
                    "aux_texture",
                    "target_texture",
                    "active_tiles",
                ],
            ),
            (
                "filter_apply_region_mask",
                &["config", "aux_texture", "target_texture", "active_tiles"],
            ),
        ],
    )
}

pub(super) fn morphology(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("source_texture", buffer(1, Kind::Texture)),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
        ],
        &[(
            "filter_morphology_axis_region",
            &["config", "source_texture", "target_texture", "active_tiles"],
        )],
    )
}

pub(super) fn displacement(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("source_texture", buffer(1, Kind::Texture)),
            ("aux_texture", buffer(2, Kind::Texture)),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
        ],
        &[(
            "filter_displacement_map_region",
            &[
                "config",
                "source_texture",
                "aux_texture",
                "target_texture",
                "active_tiles",
            ],
        )],
    )
}

pub(super) fn transfer(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("transfer_tables", buffer(7, Kind::Read)),
            ("active_tiles", buffer(8, Kind::Read)),
        ],
        &[(
            "filter_component_transfer_region",
            &[
                "config",
                "target_texture",
                "transfer_tables",
                "active_tiles",
            ],
        )],
    )
}

pub(super) fn convolve(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("source_texture", buffer(1, Kind::Texture)),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("convolve_kernels", buffer(6, Kind::Read)),
            ("active_tiles", buffer(8, Kind::Read)),
        ],
        &[(
            "filter_convolve_matrix_region",
            &[
                "config",
                "source_texture",
                "target_texture",
                "convolve_kernels",
                "active_tiles",
            ],
        )],
    )
}

pub(super) fn resample(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("source_texture", buffer(1, Kind::Texture)),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
        ],
        &[
            (
                "filter_downsample_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
            (
                "filter_upsample_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
        ],
    )
}
