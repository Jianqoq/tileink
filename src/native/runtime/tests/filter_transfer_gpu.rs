use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{Result, compute::ComputeBatch, program::filter::transfer};
use crate::shared::{
    filter_config::FilterConfig, gpu_constants::TILE_SIZE,
    layer::filter::COMPONENT_TRANSFER_TABLE_LEN,
};

use super::filter_transfer_cases::tables;

#[test]
fn transfer_tables_validate_values_indices_and_ownership() -> Result<()> {
    let mut batch = ComputeBatch::new();
    assert!(transfer::upload(&mut batch, &[]).is_err());
    let invalid = [256; COMPONENT_TRANSFER_TABLE_LEN];
    assert!(transfer::upload(&mut batch, &[invalid]).is_err());
    let table = transfer::upload(&mut batch, &tables())?;
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let c = FilterConfig {
        width: 1,
        height: 1,
        region_width: 1,
        region_height: 1,
        ..Default::default()
    };
    for index in [3, u32::MAX] {
        assert!(
            transfer::encode(
                &mut batch,
                FilterConfig {
                    table_index: index,
                    ..c
                },
                None,
                table,
                target
            )
            .is_err()
        );
    }
    let mut foreign_batch = ComputeBatch::new();
    let foreign = transfer::upload(&mut foreign_batch, &tables())?;
    assert!(transfer::encode(&mut batch, c, None, foreign, target).is_err());
    assert!(batch.passes().is_empty());
    transfer::encode(&mut batch, c, None, table, target)?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_component_transfer_tables_match_integer_semantics() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let (batch, expected) = super::filter_transfer_cases::case()?;
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "component transfer independent integer tables",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
