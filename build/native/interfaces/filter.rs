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
        count: 1,
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

pub(super) fn blur(constants: &BTreeMap<String, u32>, shared: bool) -> Interface {
    let size = if shared {
        [
            constants["SHARED_BLUR_TILE_WIDTH"],
            constants["SHARED_BLUR_TILE_HEIGHT"],
            1,
        ]
    } else {
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1]
    };
    let entry = if shared {
        "filter_blur_shared_region"
    } else {
        "filter_blur_region"
    };
    interface(
        size,
        &[
            ("config", config()),
            ("source_texture", buffer(1, Kind::Texture)),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
        ],
        &[(
            entry,
            &["config", "source_texture", "target_texture", "active_tiles"],
        )],
    )
}

pub(super) fn lighting(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("source_texture", buffer(1, Kind::Texture)),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
        ],
        &[(
            "filter_lighting_region",
            &["config", "source_texture", "target_texture", "active_tiles"],
        )],
    )
}

pub(super) fn rectangle(constants: &BTreeMap<String, u32>) -> Interface {
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
                "filter_rect_mask_region",
                &["config", "target_texture", "active_tiles"],
            ),
            (
                "filter_composite_direct_region",
                &[
                    "config",
                    "source_texture",
                    "aux_texture",
                    "target_texture",
                    "active_tiles",
                ],
            ),
            (
                "filter_composite_rect_direct_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
            (
                "filter_upsample_rect_composite_region",
                &["config", "source_texture", "target_texture", "active_tiles"],
            ),
        ],
    )
}

pub(super) fn path_mask(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
            ("path_range_starts", buffer(9, Kind::Read)),
            ("path_range_ends", buffer(10, Kind::Read)),
            ("path_p0x", buffer(11, Kind::Read)),
            ("path_p0y", buffer(12, Kind::Read)),
            ("path_p1x", buffer(13, Kind::Read)),
            ("path_p1y", buffer(14, Kind::Read)),
        ],
        &[(
            "filter_path_mask_region",
            &[
                "config",
                "target_texture",
                "active_tiles",
                "path_range_starts",
                "path_range_ends",
                "path_p0x",
                "path_p0y",
                "path_p1x",
                "path_p1y",
            ],
        )],
    )
}

pub(super) fn turbulence(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
            ("turbulence_selectors", buffer(5, Kind::Read)),
            ("turbulence_gradients", buffer(6, Kind::Read)),
        ],
        &[(
            "filter_turbulence_region",
            &[
                "config",
                "target_texture",
                "active_tiles",
                "turbulence_selectors",
                "turbulence_gradients",
            ],
        )],
    )
}

pub(super) fn surface(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("source_texture", buffer(1, Kind::Texture)),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
        ],
        &[(
            "filter_composite_surface_direct_region",
            &["config", "source_texture", "target_texture", "active_tiles"],
        )],
    )
}

pub(super) fn layer(constants: &BTreeMap<String, u32>) -> Interface {
    interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
            ("paint", buffer(10, Kind::Read)),
            ("draws", buffer(20, Kind::Read)),
            ("paths", buffer(21, Kind::Read)),
            ("backdrops", buffer(22, Kind::Read)),
            ("ranges", buffer(23, Kind::Read)),
            ("segments", buffer(24, Kind::Read)),
        ],
        &[(
            "filter_layer_mask_region",
            &[
                "config",
                "target_texture",
                "active_tiles",
                "paint",
                "draws",
                "paths",
                "backdrops",
                "ranges",
                "segments",
            ],
        )],
    )
}

pub(super) fn stack(constants: &BTreeMap<String, u32>) -> Interface {
    let mut result = layer(constants);
    for (name, slot, kind) in [
        ("source_texture", 1, Kind::Texture),
        ("aux_texture", 2, Kind::Texture),
        ("layers", 25, Kind::Read),
    ] {
        result.resources.insert(name.into(), buffer(slot, kind));
    }
    let mut common = result.entries.remove("filter_layer_mask_region").unwrap();
    common.extend(["source_texture".into(), "layers".into()]);
    result.entries.insert(
        "filter_composite_surface_stack_region".into(),
        common.clone(),
    );
    common.push("aux_texture".into());
    result
        .entries
        .insert("filter_composite_stack_region".into(), common.clone());
    result
        .entries
        .insert("filter_composite_blend_stack_region".into(), common);
    result
}

pub(super) fn glass(constants: &BTreeMap<String, u32>) -> Interface {
    let mut result = displacement(constants);
    let bindings = result
        .entries
        .remove("filter_displacement_map_region")
        .unwrap();
    result
        .entries
        .insert("filter_liquid_glass_region".into(), bindings.clone());
    result
        .entries
        .insert("filter_liquid_glass_rect_composite_region".into(), bindings);
    result
}

pub(super) fn brush(constants: &BTreeMap<String, u32>) -> Interface {
    let mut sampler = buffer(13, Kind::Sampler);
    sampler.size = 0;
    let table = super::texture::table(constants)
        .resources
        .remove("texture_table")
        .unwrap();
    let common = &[
        "config",
        "target_texture",
        "active_tiles",
        "brush_blob",
        "image_resource_atlas",
        "image_resource_sampler",
        "image_resource_textures",
    ];
    let mut result = interface(
        [constants["FILTER_WORKGROUP_SIZE"], 1, 1],
        &[
            ("config", config()),
            ("target_texture", buffer(3, Kind::TextureWrite)),
            ("active_tiles", buffer(8, Kind::Read)),
            ("brush_blob", buffer(10, Kind::Read)),
            ("image_resource_atlas", buffer(12, Kind::TextureArray)),
            ("image_resource_sampler", sampler),
            ("image_resource_textures", table),
            ("aux_texture", buffer(2, Kind::Texture)),
        ],
        &[("filter_flood_region", common)],
    );
    let mut shadow = common.iter().map(|s| (*s).into()).collect::<Vec<_>>();
    shadow.push("aux_texture".into());
    result
        .entries
        .insert("filter_composite_drop_shadow_region".into(), shadow);
    result
}
