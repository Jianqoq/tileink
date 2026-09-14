#[cfg(any(feature = "native-dx12", feature = "native-vulkan"))]
#[test]
fn native_build_embeds_each_expected_nonempty_program() {
    let artifacts = tileink::NATIVE_SHADER_ARTIFACTS;
    let dxil = cfg!(all(feature = "native-dx12", target_os = "windows"));
    let spirv = cfg!(all(
        feature = "native-vulkan",
        any(target_os = "windows", target_os = "linux")
    ));
    let entries = [
        "coarse_emit",
        "coarse_emit_bins",
        "coarse_emit_chunks",
        "coarse_count",
        "coarse_count_bins",
        "coarse_emit_chunk_particle_counts",
        "coarse_emit_chunk_particle_offsets",
        "coarse_tile_counts_from_emit_chunks",
        "coarse_emit_chunk_tile_kinds",
        "filter_clear_region",
        "filter_flood_region",
        "filter_composite_drop_shadow_region",
        "filter_blend_region",
        "filter_morphology_axis_region",
        "filter_displacement_map_region",
        "filter_component_transfer_region",
        "filter_convolve_matrix_region",
        "filter_downsample_region",
        "filter_upsample_region",
        "filter_blur_region",
        "filter_lighting_region",
        "filter_rect_mask_region",
        "filter_path_mask_region",
        "filter_turbulence_region",
        "filter_composite_surface_direct_region",
        "filter_layer_mask_region",
        "filter_composite_stack_region",
        "filter_liquid_glass_region",
        "filter_liquid_glass_rect_composite_region",
        "filter_composite_blend_stack_region",
        "filter_composite_surface_stack_region",
        "sdf_coverage_words",
        "filter_composite_direct_region",
        "filter_composite_rect_direct_region",
        "filter_upsample_rect_composite_region",
        "filter_blur_shared_region",
        "filter_composite_inputs_region",
        "filter_apply_region_mask",
        "filter_color_region",
        "filter_color_matrix_region",
        "filter_copy_region",
        "filter_source_alpha_region",
        "filter_source_over_region",
        "filter_svg_mask_coverage_region",
        "filter_tile_region",
        "filter_offset_region",
        "filter_drop_shadow_mask_region",
        "gradient_words",
        "brush_words",
        "text_words",
        "fine_tile_main",
        "pattern_words",
        "texture_table_words",
        "texture_flip",
        "texture_layer",
        "sampler_words",
        "blend_math_words",
        "fill_coverage_words",
        "geometry_math_words",
        "pixel_math_words",
        "clear_words",
        "copy_words",
        "layout_words",
        "sample_words",
        "range_scatter",
        "cumsum_prefix_chunks",
        "cumsum_chunk_offsets",
        "cumsum_apply_chunk_offsets",
        "coarse_emit_chunk_counts",
        "coarse_emit_prefix_chunks",
        "coarse_emit_chunk_offsets",
        "coarse_emit_apply_chunk_offsets",
        "coarse_emit_fill_refs",
        "coarse_prefix_chunks",
        "coarse_chunk_offsets",
        "coarse_apply_chunk_offsets",
        "scan_clear",
        "scan_count",
        "scan_emit",
        "scan_prefix_chunks",
        "scan_chunk_offsets",
        "scan_apply_chunk_offsets",
    ];
    assert_eq!(
        artifacts.len(),
        entries.len() * (usize::from(dxil) + usize::from(spirv))
    );
    for format in ["dxil", "spirv"] {
        if (format == "dxil" && !dxil) || (format == "spirv" && !spirv) {
            continue;
        }
        for entry in entries {
            let matches: Vec<_> = artifacts
                .iter()
                .filter(|a| a.format == format && a.entry == entry)
                .collect();
            assert_eq!(matches.len(), 1);
            let artifact = matches[0];
            assert!(!artifact.bytes.is_empty());
            assert_eq!(artifact.cache_key.len(), 64);
            assert!(artifact.cache_key.bytes().all(|b| b.is_ascii_hexdigit()));
            assert_eq!(
                &artifact.bytes[..4],
                if format == "dxil" {
                    b"DXBC"
                } else {
                    &[3, 2, 35, 7]
                }
            );
        }
    }
}
