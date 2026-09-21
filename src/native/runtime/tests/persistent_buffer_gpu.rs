use crate::native::runtime::{Result, buffer::Buffer, compute::ComputeBatch};
use crate::{NativeContext, NativeContextOptions};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_persistent_buffers_patch_ranges_and_keep_inflight_allocations_alive() -> Result<()> {
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let backend = super::backend();
    let options = NativeContextOptions {
        physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
        validation: true,
    };
    let context = NativeContext::new(backend, &options)?;
    let buffer = Buffer::new(&context.adapter, 16)?;
    let initial: Vec<u8> = [11u32, 23, 37, 51]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let mut first = ComputeBatch::new();
    assert!(first.import_buffer(&buffer, &[]).is_err());
    assert!(first.resources().is_empty());
    let id = first.import_buffer(&buffer, &[(0, &initial)])?;
    first.readback(id)?;
    let other = NativeContext::new(backend, &options)?;
    assert!(other.adapter.submit_compute(&first).is_err());
    assert!(!buffer.state.initialized.get());
    let first_receipt = context
        .adapter
        .submit_compute(&first)
        .map_err(|error| format!("{error:?}"))?;
    assert!(buffer.state.initialized.get());
    drop(first);
    let mut second = ComputeBatch::new();
    assert!(second.import_buffer(&buffer, &[(1, &[0; 4])]).is_err());
    assert!(second.import_buffer(&buffer, &[(16, &[0; 4])]).is_err());
    assert!(second.resources().is_empty());
    let patch = 97u32.to_le_bytes();
    let id = second.import_buffer(&buffer, &[(4, &patch)])?;
    assert!(second.import_buffer(&buffer, &[]).is_err());
    second.readback(id)?;
    let second_receipt = context
        .adapter
        .submit_compute(&second)
        .map_err(|error| format!("{error:?}"))?;
    drop(second);
    let mut last = ComputeBatch::new();
    let id = last.import_buffer(&buffer, &[])?;
    last.readback(id)?;
    let last_receipt = context
        .adapter
        .submit_compute(&last)
        .map_err(|error| format!("{error:?}"))?;
    drop(last);
    drop(buffer);
    let mut expected = initial.clone();
    expected[4..8].copy_from_slice(&patch);
    assert_eq!(last_receipt.readback()?, vec![expected.clone()]);
    assert_eq!(second_receipt.readback()?, vec![expected]);
    assert_eq!(first_receipt.readback()?, vec![initial]);
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_cached_buffer_rebuilds_after_discarded_dirty_upload() -> Result<()> {
    use crate::native::runtime::{
        compute::{Resource, SurfacePool},
        program::cached_buffer::CachedBuffer,
    };
    use std::{cell::RefCell, rc::Rc};
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let backend = super::backend();
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut cache = CachedBuffer::default();
    let mut batch = ComputeBatch::with_surfaces(pool.clone());
    cache.upload(&mut batch, &[1u32, 2, 3, 4], None)?;
    context.submit_compute(&batch)?.wait()?;
    drop(batch);
    let mut rejected = ComputeBatch::with_surfaces(pool.clone());
    let dirty = std::iter::once(1..2).collect::<Vec<_>>();
    let id = cache.upload(&mut rejected, &[1u32, 7, 3, 4], Some(&dirty))?;
    assert_eq!(rejected.resources()[id.index()].bytes().len(), 4);
    drop(rejected);
    let mut retry = ComputeBatch::with_surfaces(pool.clone());
    let dirty = std::iter::once(2..3).collect::<Vec<_>>();
    let data = [1u32, 7, 9, 4];
    let id = cache.upload(&mut retry, &data, Some(&dirty))?;
    assert_eq!(retry.resources()[id.index()].bytes().len(), 16);
    retry.readback(id)?;
    let receipt = context
        .adapter
        .submit_compute(&retry)
        .map_err(|error| format!("{error:?}"))?;
    drop(retry);
    let mut unchanged = ComputeBatch::with_surfaces(pool);
    let id = cache.upload(&mut unchanged, &data, Some(&[]))?;
    assert!(
        matches!(&unchanged.resources()[id.index()], Resource::PersistentBuffer(upload) if upload.bytes.is_empty())
    );
    unchanged.readback(id)?;
    let final_receipt = context
        .adapter
        .submit_compute(&unchanged)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        final_receipt.readback()?,
        vec![bytemuck::cast_slice::<_, u8>(&data).to_vec()]
    );
    assert_eq!(
        receipt.readback()?,
        vec![bytemuck::cast_slice::<_, u8>(&data).to_vec()]
    );
    context.check_validation()?;
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_fragmented_uploads_preserve_holes_and_queued_readbacks() -> Result<()> {
    let context = NativeContext::new(
        super::backend(),
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: false,
        },
    )?;
    let buffer = Buffer::new(&context.adapter, 512)?;
    let initial = vec![0xa5; 512];
    let mut initialization = ComputeBatch::new();
    initialization.import_buffer(&buffer, &[(0, &initial)])?;
    context.submit_compute(&initialization)?.wait()?;
    let mut expected = initial;
    let mut receipts = Vec::new();
    for phase in 0..2u32 {
        let values: Vec<_> = (0..32u32)
            .map(|i| (i + phase * 100).to_le_bytes())
            .collect();
        let updates: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(i, value)| (i * 12 + phase as usize * 4, value.as_slice()))
            .collect();
        let mut batch = ComputeBatch::new();
        let id = batch.import_buffer(&buffer, &updates)?;
        #[cfg(feature = "dx12")]
        assert_eq!(
            batch
                .passes()
                .iter()
                .filter(|pass| pass.shader.entry == "range_scatter")
                .count(),
            1
        );
        for &(offset, bytes) in &updates {
            expected[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
        batch.readback(id)?;
        let receipt = context
            .adapter
            .submit_compute(&batch)
            .map_err(|error| format!("{error:?}"))?;
        receipts.push((receipt, expected.clone()));
    }
    drop(buffer);
    for (receipt, expected) in receipts.into_iter().rev() {
        assert_eq!(receipt.readback()?, vec![expected]);
    }
    Ok(())
}
