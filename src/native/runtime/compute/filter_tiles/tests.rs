use super::*;
use crate::native::runtime::compute::Resource;

#[test]
fn validation_proofs_are_batch_owned_and_do_not_allocate_gpu_resources() -> Result<()> {
    let source = ComputeBatch::new();
    let mut other = ComputeBatch::new();
    let tiles = [7, 0, 3];
    let proof = source.validate_filter_tiles(&tiles, 8)?;
    assert!(source.resources().is_empty());
    assert!(other.filter_tile_buffer(proof).is_err());
    assert!(other.resources().is_empty());
    Ok(())
}

#[test]
fn independently_prepared_proofs_share_one_uploaded_list() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let tiles = [7, 0, 3];
    let first = batch.validate_filter_tiles(&tiles, 8)?;
    let second = batch.validate_filter_tiles(&tiles, 8)?;
    let buffer = batch.filter_tile_buffer(first)?;
    assert_eq!(batch.filter_tile_buffer(second)?, buffer);
    assert_eq!(batch.resources().len(), 1);
    let another = batch.validate_filter_tiles(&tiles, 16)?;
    assert_eq!(batch.filter_tile_buffer(another)?, buffer);
    assert!(batch.validate_filter_tiles(&tiles, 7).is_err());
    assert_eq!(batch.resources().len(), 1);
    Ok(())
}

#[test]
fn empty_lists_and_maximum_u32_tile_ids_keep_checked_range_semantics() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let empty = batch.validate_filter_tiles(&[], 0)?;
    let buffer = batch.filter_tile_buffer(empty)?;
    assert!(
        matches!(&batch.resources()[buffer.index()], Resource::Buffer(bytes) if bytes == &[0;4])
    );
    let cached = batch.validate_filter_tiles(&[], 0)?;
    assert_eq!(batch.filter_tile_buffer(cached)?, buffer);
    assert!(batch.validate_filter_tiles(&[0], 0).is_err());
    assert!(
        batch
            .validate_filter_tiles(&[u32::MAX], u64::from(u32::MAX))
            .is_err()
    );
    let maximum = batch.validate_filter_tiles(&[u32::MAX], u64::from(u32::MAX) + 1)?;
    let maximum = batch.filter_tile_buffer(maximum)?;
    assert!(
        matches!(&batch.resources()[maximum.index()], Resource::Buffer(bytes) if bytes == &u32::MAX.to_le_bytes())
    );
    assert!(
        batch
            .validate_filter_tiles(&[u32::MAX], u64::from(u32::MAX))
            .is_err()
    );
    assert!(batch.validate_filter_tiles(&[1, 1], 2).is_err());
    Ok(())
}

#[test]
fn owned_keys_detect_reused_input_storage_and_keep_old_gpu_bytes_immutable() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let mut tiles = vec![7u32, 0, 3];
    let proof = batch.validate_filter_tiles(&tiles, 8)?;
    let first = batch.filter_tile_buffer(proof)?;
    tiles[2] = 0;
    assert!(batch.validate_filter_tiles(&tiles, 8).is_err());
    tiles[2] = 2;
    let proof = batch.validate_filter_tiles(&tiles, 8)?;
    let second = batch.filter_tile_buffer(proof)?;
    assert_ne!(first, second);
    let Resource::Buffer(bytes) = &batch.resources()[first.index()] else {
        panic!("owned tile buffer")
    };
    let expected: Vec<_> = [7u32, 0, 3]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert_eq!(bytes, &expected);
    let fresh = ComputeBatch::new();
    assert!(fresh.size(first).is_err());
    Ok(())
}
