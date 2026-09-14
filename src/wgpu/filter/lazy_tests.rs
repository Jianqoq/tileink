use super::*;

use crate::wgpu::test_gpu as explicit_gpu;

#[test]
fn filter_modules_share_remaps_without_eager_or_repeated_compilation() {
    if std::env::var("TILEINK_RUN_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }
    let api = std::env::var("TILEINK_TEST_API").expect("explicit GPU API required");
    let portable = std::env::var("TILEINK_WGPU_MODE").as_deref() == Ok("portable");
    // Two independent owners/devices must never share a module. Run each texture
    // mode and API in a separate process so failures cannot select a fallback.
    for _ in 0..2 {
        let (_, device, _queue) =
            explicit_gpu::device(&api, portable, false, ::wgpu::MemoryHints::Performance);
        let tracker = PipelineCompilationTracker::default();
        let filters = WgpuFilterPipeline::new(&device, None, &tracker).unwrap();
        // Every production kernel has exactly one slot, including resource-rich
        // entries that are not needed by this first-filter compilation sequence.
        for kernel in [
            &filters.clear_region,
            &filters.copy_region,
            &filters.source_alpha_region,
            &filters.source_over_region,
            &filters.tile_region,
            &filters.offset_region,
            &filters.flood_region,
            &filters.drop_shadow_mask_region,
            &filters.morphology_axis_region,
            &filters.downsample_region,
            &filters.upsample_region,
            &filters.upsample_rect_composite_region,
            &filters.blur_region,
            &filters.blur_shared_region,
            &filters.svg_mask_coverage_region,
            &filters.apply_region_mask,
            &filters.color_filter_region,
            &filters.color_matrix_region,
            &filters.component_transfer_region,
            &filters.convolve_matrix_region,
            &filters.lighting_region,
            &filters.liquid_glass_region,
            &filters.liquid_glass_rect_composite_region,
            &filters.blend_region,
            &filters.composite_inputs_region,
            &filters.displacement_map_region,
            &filters.turbulence_region,
            &filters.composite_drop_shadow_region,
            &filters.layer_mask_region,
            &filters.rect_mask_region,
            &filters.path_mask_region,
            &filters.composite_direct_region,
            &filters.composite_rect_direct_region,
            &filters.composite_stack_region,
            &filters.composite_blend_stack_region,
            &filters.composite_surface_direct_region,
            &filters.composite_surface_stack_region,
        ] {
            assert_eq!(
                filters
                    .shader_modules
                    .iter()
                    .filter(|(mask, _)| *mask == kernel.resources)
                    .count(),
                1,
                "missing or duplicate module slot for {}",
                kernel.entry_point,
            );
        }
        let modules = || filters.created_shader_modules.load(Ordering::Relaxed);
        assert_eq!(modules(), 0);
        assert_eq!(tracker.epoch(), 0);
        for (index, kernel) in [
            &filters.clear_region,
            &filters.copy_region,
            &filters.blur_shared_region,
            &filters.composite_direct_region,
        ]
        .into_iter()
        .enumerate()
        {
            filters.kernel(&device, kernel);
            assert_eq!(modules(), 1, "identical final WGSL must share one module");
            assert_eq!(tracker.epoch(), index as u64 + 1);
            filters.kernel(&device, kernel);
            assert_eq!(modules(), 1);
            assert_eq!(tracker.epoch(), index as u64 + 1);
        }
        // Different resource remaps must remain separate even though the
        // unpatched WGSL source is the same; this also validates the layouts.
        filters.kernel(&device, &filters.component_transfer_region);
        assert_eq!(modules(), 2);
        assert_eq!(tracker.epoch(), 5);
        filters.kernel(&device, &filters.component_transfer_region);
        assert_eq!(modules(), 2);
        assert_eq!(tracker.epoch(), 5);
    }
}
