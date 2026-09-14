use super::four_api::Routes;
use crate::native::runtime::{Result, compute::ComputeBatch};
use crate::shared::{fine_config::FineConfig, gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_fine_tile_selection_preserves_logical_bounds_and_incremental_pixels() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
    )?;
    for incremental in [false, true] {
        for load_target in [false, true] {
            let mut batch = ComputeBatch::new();
            let mut coarse = vec![0u32; 6 * 6];
            for tile in 0..6 {
                coarse[tile * 6 + 1] = (tile * 2) as u32;
                coarse[tile * 6 + 2] = (tile * 2 + 2) as u32;
                // A deliberately stale COLOR specialization must restart the
                // full interpreter when it encounters a fill particle.
                coarse.extend(if tile == 2 {
                    [1, 1, 0, 0, 0, 0]
                } else {
                    [2, 0, 0, 0, 0, 0xffab371d]
                });
                coarse.extend([0; 6]);
            }
            let kind_base = coarse.len() as u32;
            coarse.extend([0, 1, 2, 3, 4, 5]);
            let list_base = coarse.len() as u32;
            coarse.extend([5, 0, 2]);
            let config = batch.buffer(
                bytemuck::bytes_of(&FineConfig {
                    width: 33,
                    height: 19,
                    tiles_width: 3,
                    tiles_height: 2,
                    tile_count: 6,
                    clear_color: 0xff193d57,
                    load_target: u32::from(load_target),
                    ptcl_capacity: 12,
                    fine_tile_kind_base: kind_base,
                    active_tile_count: if incremental { 3 } else { 6 },
                    dispatch_width: 2,
                    active_tile_list_base: list_base,
                    incremental: u32::from(incremental),
                    ..Default::default()
                })
                .to_vec(),
            )?;
            let target = batch.texture_rgba8([35, 21], [41u8, 53, 67, 255].repeat(35 * 21))?;
            let draws = batch.buffer(vec![
                0;
                std::mem::size_of::<
                    crate::shared::draw_record::DrawRecord,
                >()
            ])?;
            let mut brush = vec![0u32; 21];
            brush[0] =
                super::hlsl_constants::read_hlsl("shared/brush/constants.hlsli")?["BRUSH_SOLID"];
            brush[4] = 0xff1d37ab;
            let paint = batch.buffer(brush.into_iter().flat_map(u32::to_le_bytes).collect())?;
            let coarse = batch.buffer(coarse.into_iter().flat_map(u32::to_le_bytes).collect())?;
            let segments = batch.buffer(vec![
                0;
                std::mem::size_of::<
                    crate::shared::line_seg::LineSegment,
                >()
            ])?;
            let text = batch.buffer(vec![0; 4])?;
            let spills = batch.buffer(vec![0; 4])?;
            let atlas = batch.texture_array_rgba8([1, 1, 1], vec![0; 4])?;
            let image = batch.texture_rgba8([1, 1], vec![0; 4])?;
            let images =
                batch.texture_table(&vec![image; NATIVE_TEXTURE_TABLE_CAPACITY as usize])?;
            let sampler = batch.sampler(crate::native::runtime::compute::SamplerFilter::Linear)?;
            // SAFETY: complete tile/particle tables, COLOR/END plus one solid
            // FILL with a full-tile backdrop and an empty segment range;
            // valid compact tile indices and full output allocation. Extra groups
            // and edge pixels must return before touching an inactive target pixel.
            unsafe {
                batch.dispatch(
                    "fine_tile_main",
                    &[
                        (0, config),
                        (1, target),
                        (2, draws),
                        (3, paint),
                        (4, coarse),
                        (5, segments),
                        (6, text),
                        (7, spills),
                        (12, atlas),
                        (13, sampler),
                        (30, images),
                    ],
                    [2, 4, 1],
                )?;
            }
            batch.readback(target)?;
            let mut expected = [41u8, 53, 67, 255].repeat(35 * 21);
            for y in 0..19 {
                for x in 0..33 {
                    let tile = x / 16 + y / 16 * 3;
                    if incremental && ![5, 0, 2].contains(&tile) {
                        continue;
                    }
                    let color = if tile == 1 {
                        if load_target {
                            0xff433529u32
                        } else {
                            0xff193d57
                        }
                    } else if tile == 2 {
                        0xff1d37ab
                    } else {
                        0xffab371d
                    };
                    expected[(y * 35 + x) * 4..(y * 35 + x + 1) * 4]
                        .copy_from_slice(&color.to_le_bytes());
                }
            }
            for variant in super::reference::FineVariant::ALL {
                routes.check_fine(
                    &batch,
                    std::slice::from_ref(&expected),
                    &format!("fine selection and padded dispatch {variant:?}"),
                    variant,
                )?;
            }
        }
    }
    super::fine_images::check(&routes)?;
    routes.assert_reference_pipeline_builds(super::reference::FineVariant::ALL.len());
    routes.validate()
}
