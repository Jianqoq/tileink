use super::{four_api::Routes, reference::FilterVariant};
use crate::{
    native::runtime::{Result, compute::ComputeBatch},
    shared::{
        filter_config::FilterConfig,
        gpu_constants::{FILTER_WORKGROUP_SIZE, SDF_PROBE_REQUEST_WORDS, SDF_RECORD_WORDS},
    },
};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_sdf_shapes_transforms_and_boundaries() -> Result<()> {
    let (records, requests, oracle) = sdf_cases::cases();
    let checked = oracle.len();
    let bytes = |v: &[u32]| v.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>();
    let count = (requests.len() / SDF_PROBE_REQUEST_WORDS as usize) as u32;
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(
        bytemuck::bytes_of(&FilterConfig {
            pixel_count: count,
            ..Default::default()
        })
        .to_vec(),
    )?;
    let paint = batch.buffer(bytes(&records))?;
    let positions = batch.buffer(bytes(&requests))?;
    let output = batch.buffer(bytes(&vec![0xa1b2c3d4; count as usize + 4]))?;
    // SAFETY: each request addresses a complete 17-word shape; logical count
    // excludes four output sentinels and extra dispatch groups exercise tail guards.
    unsafe {
        batch.dispatch(
            "sdf_coverage_words",
            &[(0, config), (5, positions), (6, output), (7, paint)],
            [count.div_ceil(FILTER_WORKGROUP_SIZE) + 1, 1, 1],
        )?;
    }
    batch.readback(output)?;
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let variant = FilterVariant {
        portable: false,
        texture_table: false,
    };
    let expected = routes.filter_reference_output(&batch, variant)?;
    assert_eq!(&expected[0][..checked * 4], bytes(&oracle));
    assert_eq!(&expected[0][count as usize * 4..], bytes(&[0xa1b2c3d4; 4]));
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "SDF geometry and transformed antialiasing",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}

#[path = "../../../../tests/shaders/sdf_cases.rs"]
mod sdf_cases;
