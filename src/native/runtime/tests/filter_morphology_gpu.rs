use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{Result, compute::ComputeBatch, program::filter::morphology};
use crate::shared::filter_config::FilterConfig;

#[test]
fn morphology_rejects_invalid_axis_operator_and_overflow() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([3, 5], vec![0; 60])?;
    let target = batch.texture_rgba8([3, 5], vec![0; 60])?;
    let c = FilterConfig {
        width: 3,
        height: 5,
        region_width: 3,
        region_height: 5,
        ..Default::default()
    };
    for invalid in [
        FilterConfig {
            morphology_axis: 2,
            ..c
        },
        FilterConfig {
            morphology_operator: 2,
            ..c
        },
        FilterConfig {
            morphology_radius: u32::MAX,
            ..c
        },
    ] {
        assert!(morphology::encode(&mut batch, invalid, None, source, target).is_err());
    }
    assert!(batch.passes().is_empty());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_morphology_matches_rational_straight_channel_semantics() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    for (batch, expected) in super::filter_morphology_cases::cases()? {
        for portable in [false, true] {
            for texture_table in [false, true] {
                routes.check_variant(
                    &batch,
                    &expected,
                    "morphology rational semantics",
                    Some(FilterVariant {
                        portable,
                        texture_table,
                    }),
                )?;
            }
        }
    }
    routes.validate()
}
