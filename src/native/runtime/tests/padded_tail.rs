//! Allocated metadata capacity is not the logical dispatch length.
use super::{Result, reference};
use crate::{
    NativeBackend,
    native::runtime::{adapter::Adapter, compute::ComputeBatch},
    shared::gpu_plan::GpuScanChunk,
};
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn compare(batch: ComputeBatch, expected: Vec<Vec<u8>>) -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    for native in [&dx12, &vulkan] {
        let receipt = native
            .submit_compute(&batch)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(receipt.readback()?, expected);
    }
    for reference in &references {
        assert_eq!(reference.execute_compute(&batch)?, expected);
    }
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn scan_padded_metadata_capacity_cannot_touch_an_inactive_chunk() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(words(&[0, 6, 0, 5, 0, 0, 1, 0, 0, 0, 0]))?;
    let metadata: Vec<_> = (0..6)
        .map(|i| GpuScanChunk {
            path_id: 0,
            backdrop_offset: i,
            segment_start: 11,
            len: 1,
        })
        .collect();
    let chunks = batch.buffer(bytemuck::cast_slice(&metadata).to_vec())?;
    // Five active chunks plus stale capacity naming the inactive slot zero.
    let active = batch.buffer(words(&[1, 2, 3, 4, 5, 0]))?;
    let ranges = batch.buffer(words(&[0x55555555; 12]))?;
    let counts = batch.buffer(words(&[3; 6]))?;
    let totals = batch.buffer(words(&[0x55555555; 6]))?;
    let offsets = batch.buffer(words(&[11; 6]))?;
    let cursors = batch.buffer(words(&[0x55555555; 6]))?;
    // SAFETY: all six capacity records are addressable, but only the five
    // logical chunks own writes. The sixth workgroup must exit uniformly.
    unsafe {
        batch.dispatch(
            "scan_prefix_chunks",
            &[
                (0, config),
                (1, chunks),
                (2, ranges),
                (3, counts),
                (4, totals),
                (5, active),
            ],
            [3, 2, 1],
        )?;
        batch.dispatch(
            "scan_apply_chunk_offsets",
            &[
                (0, config),
                (1, chunks),
                (2, ranges),
                (3, cursors),
                (4, offsets),
                (5, active),
            ],
            [3, 2, 1],
        )?;
    }
    for id in [ranges, totals, cursors] {
        batch.readback(id)?;
    }
    let mut expected_ranges = vec![0x55555555; 2];
    for _ in 0..5 {
        expected_ranges.extend([11, 14]);
    }
    let mut expected_totals = vec![0x55555555];
    expected_totals.extend([3; 5]);
    let mut expected_cursors = vec![0x55555555];
    expected_cursors.extend([11; 5]);
    compare(
        batch,
        vec![
            words(&expected_ranges),
            words(&expected_totals),
            words(&expected_cursors),
        ],
    )
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn cumsum_padded_metadata_capacity_cannot_execute_stale_chunks() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(words(&[1, 5, 0, 0]))?;
    let offsets = batch.buffer(words(&[0, 1, 2, 3, 4, 5]))?;
    let lengths = batch.buffer(words(&[1; 6]))?;
    let backdrops = batch.buffer(words(&[3; 6]))?;
    let totals = batch.buffer(words(&[0x55555555; 6]))?;
    let carries = batch.buffer(words(&[11; 6]))?;
    // SAFETY: five logical chunks, six allocated records, disjoint destinations.
    unsafe {
        batch.dispatch(
            "cumsum_prefix_chunks",
            &[
                (0, config),
                (1, offsets),
                (2, lengths),
                (5, backdrops),
                (6, totals),
            ],
            [3, 2, 1],
        )?;
        batch.dispatch(
            "cumsum_apply_chunk_offsets",
            &[
                (0, config),
                (1, offsets),
                (2, lengths),
                (5, backdrops),
                (7, carries),
            ],
            [3, 2, 1],
        )?;
    }
    batch.readback(backdrops)?;
    batch.readback(totals)?;
    compare(
        batch,
        vec![
            words(&[14, 14, 14, 14, 14, 3]),
            words(&[3, 3, 3, 3, 3, 0x55555555]),
        ],
    )
}
