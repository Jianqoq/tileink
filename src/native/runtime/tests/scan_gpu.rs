//! Scan prefix intermediate values are compared without tolerances or reordering.
use super::{Result, reference};
use crate::{
    NativeBackend,
    native::runtime::{adapter::Adapter, compute::ComputeBatch},
    shared::{
        gpu_plan::{GpuScanChunk, GpuScanChunkRange},
        path::PathRecord,
    },
};

fn prefix_case(lengths: &[u32], seed: u32) -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
    let words = |values: &[u32]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let mut chunks = Vec::new();
    let mut counts = vec![0x98765432; 7];
    let mut cursor = 7usize;
    let mut rng = seed;
    for &len in lengths {
        chunks.push(GpuScanChunk {
            path_id: 0,
            backdrop_offset: cursor as u32,
            segment_start: 19,
            len,
        });
        for _ in 0..len {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            counts.push(rng);
        }
        counts.extend([0x98765432; 3]);
        cursor = counts.len();
    }
    let mut ranges = vec![0x55555555; counts.len() * 2];
    let mut cursors = vec![0x55555555; counts.len()];
    let mut totals = Vec::new();
    let mut offsets = Vec::new();
    let mut next = 19u32;
    for chunk in &chunks {
        offsets.push(next);
        let mut total = 0u32;
        for i in chunk.backdrop_offset..chunk.backdrop_offset + chunk.len {
            let count = counts[i as usize];
            ranges[i as usize * 2] = next.wrapping_add(total);
            cursors[i as usize] = next.wrapping_add(total);
            total = total.wrapping_add(count);
            ranges[i as usize * 2 + 1] = next.wrapping_add(total);
        }
        totals.push(total);
        next = next.wrapping_add(total);
    }
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(words(&[
        counts.len() as u32,
        counts.len() as u32,
        2,
        lengths.len() as u32,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]))?;
    let chunks_id = batch.buffer(bytemuck::cast_slice(&chunks).to_vec())?;
    let paths = [
        PathRecord {
            path_id: 0,
            data_offset: 7,
            data_len: counts.len() as u32 - 7,
            segment_start: 19,
            ..Default::default()
        },
        PathRecord {
            path_id: 1,
            ..Default::default()
        },
    ];
    let paths_id = batch.buffer(bytemuck::cast_slice(&paths).to_vec())?;
    let chunk_ranges = [
        GpuScanChunkRange {
            start: 0,
            end: lengths.len() as u32,
        },
        GpuScanChunkRange {
            start: lengths.len() as u32,
            end: lengths.len() as u32,
        },
    ];
    let chunk_ranges_id = batch.buffer(bytemuck::cast_slice(&chunk_ranges).to_vec())?;
    let active = batch.buffer(words(&[0]))?;
    let counts_id = batch.buffer(words(&counts))?;
    let ranges_id = batch.buffer(words(&vec![0x55555555; ranges.len()]))?;
    let cursors_id = batch.buffer(words(&vec![0x55555555; cursors.len()]))?;
    let totals_id = batch.buffer(words(&vec![0; lengths.len()]))?;
    let offsets_id = batch.buffer(words(&vec![0; lengths.len()]))?;
    let bumps_id = batch.buffer(words(&[0x55555555; 2]))?;
    let x = 3u32.min(lengths.len() as u32);
    let grid = [x, (lengths.len() as u32).div_ceil(x), 1];
    // SAFETY: dense metadata contains disjoint chunks of at most 256 words,
    // two valid paths and a complete chunk partition. Extra grid groups are tails.
    unsafe {
        batch.dispatch(
            "scan_prefix_chunks",
            &[
                (0, config),
                (1, chunks_id),
                (2, ranges_id),
                (3, counts_id),
                (4, totals_id),
                (5, active),
            ],
            grid,
        )?;
        batch.dispatch(
            "scan_chunk_offsets",
            &[
                (0, config),
                (1, paths_id),
                (2, chunk_ranges_id),
                (3, bumps_id),
                (4, totals_id),
                (5, offsets_id),
                (6, active),
            ],
            [1, 1, 1],
        )?;
        batch.dispatch(
            "scan_apply_chunk_offsets",
            &[
                (0, config),
                (1, chunks_id),
                (2, ranges_id),
                (3, cursors_id),
                (4, offsets_id),
                (5, active),
            ],
            grid,
        )?;
    }
    for id in [
        ranges_id, cursors_id, totals_id, offsets_id, bumps_id, counts_id,
    ] {
        batch.readback(id)?;
    }
    Ok((
        batch,
        vec![
            words(&ranges),
            words(&cursors),
            words(&totals),
            words(&offsets),
            words(&[next.wrapping_sub(19), 0]),
            words(&counts),
        ],
    ))
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_scan_prefix_matches_cpu_ranges_offsets_and_guards() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    for repetition in 0..3 {
        for lengths in [
            vec![0],
            vec![1],
            vec![255],
            vec![256],
            vec![256, 1],
            vec![256, 0, 17, 1, 255],
        ] {
            let (batch, expected) = prefix_case(&lengths, 17 + repetition)?;
            let d = dx12.submit_compute(&batch).map_err(|e| format!("{e:?}"))?;
            let v = vulkan
                .submit_compute(&batch)
                .map_err(|e| format!("{e:?}"))?;
            assert_eq!(d.readback()?, expected, "DX12 {lengths:?}");
            assert_eq!(v.readback()?, expected, "Vulkan {lengths:?}");
            for reference in &references {
                assert_eq!(
                    reference.execute_compute(&batch)?,
                    expected,
                    "wgpu {lengths:?}"
                );
            }
        }
    }
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    Ok(())
}

fn clear_case(backdrop_count: u32, sparse: bool) -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
    let words = |values: &[u32]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let mut batch = ComputeBatch::new();
    let path_count = if backdrop_count == 0 { 0 } else { 2 };
    let chunk_count = if backdrop_count == 0 { 0 } else { 3 };
    let count = backdrop_count.max(path_count).max(chunk_count);
    let backdrop_slots = backdrop_count as usize * 2 + 7;
    let backdrops: Vec<_> = (0..backdrop_count)
        .map(|i| if sparse { i * 2 + 3 } else { i })
        .collect();
    let paths = if sparse { vec![4, 1] } else { vec![0, 1] };
    let chunks = if sparse { vec![6, 2, 0] } else { vec![0, 1, 2] };
    let mut indices = vec![0xFFFFFFFF; 3];
    indices.extend(&paths);
    indices.extend(&chunks);
    indices.extend(&backdrops);
    let active = batch.buffer(words(&indices))?;
    let config = batch.buffer(words(&[
        count,
        backdrop_count,
        path_count,
        chunk_count,
        0,
        0,
        u32::from(sparse),
        0,
        3,
        5,
        8,
    ]))?;
    let mut ids = Vec::new();
    let mut expected = Vec::new();
    for (slot_count, stride, selected) in [
        (backdrop_slots, 1, &backdrops[..]),
        (backdrop_slots, 2, &backdrops[..]),
        (backdrop_slots, 1, &backdrops[..]),
        (backdrop_slots, 1, &backdrops[..]),
        (5, 1, &paths[..path_count as usize]),
        (7, 1, &chunks[..chunk_count as usize]),
        (7, 1, &chunks[..chunk_count as usize]),
    ] {
        let mut contents: Vec<_> = (0..slot_count * stride)
            .map(|i| 0x77770000 + i as u32)
            .collect();
        ids.push(batch.buffer(words(&contents))?);
        for &slot in selected {
            contents[slot as usize * stride..(slot as usize + 1) * stride].fill(0);
        }
        expected.push(words(&contents));
    }
    let mut bindings = vec![(0, config)];
    bindings.extend(ids.iter().enumerate().map(|(i, id)| (i as u32 + 1, *id)));
    bindings.push((8, active));
    // SAFETY: each clear index is unique in its destination class, all physical
    // indices fit their allocations and selected metadata spans fit `indices`.
    unsafe {
        batch.dispatch(
            "scan_clear",
            &bindings,
            [
                count
                    .div_ceil(crate::shared::gpu_constants::SCAN_CHUNK_SIZE)
                    .max(1),
                1,
                1,
            ],
        )?;
    }
    for id in ids {
        batch.readback(id)?;
    }
    Ok((batch, expected))
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_scan_clear_preserves_unselected_records_and_dispatch_tail() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    for repetition in 0..3 {
        for count in [0, 1, 17, 255, 256, 257] {
            for sparse in [false, true] {
                let (batch, expected) = clear_case(count, sparse)?;
                let d = dx12.submit_compute(&batch).map_err(|e| format!("{e:?}"))?;
                let v = vulkan
                    .submit_compute(&batch)
                    .map_err(|e| format!("{e:?}"))?;
                assert_eq!(
                    d.readback()?,
                    expected,
                    "DX12 count {count} sparse {sparse} repeat {repetition}"
                );
                assert_eq!(
                    v.readback()?,
                    expected,
                    "Vulkan count {count} sparse {sparse} repeat {repetition}"
                );
                for reference in &references {
                    assert_eq!(reference.execute_compute(&batch)?, expected);
                }
            }
        }
    }
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    Ok(())
}

#[test]
fn scan_record_abi_matches_the_shared_host_records() {
    use crate::shared::{line::Line, line_seg::LineSegment, tile_seg_range::TileSegmentRange};
    use std::mem::{offset_of, size_of};
    let abi: serde_json::Value =
        serde_json::from_str(include_str!("../../../shaders/scan-records-abi.json")).unwrap();
    let expected = [
        ("SCAN_PATH_RECORD_STRIDE", size_of::<PathRecord>()),
        (
            "SCAN_PATH_SEGMENT_START",
            offset_of!(PathRecord, segment_start),
        ),
        ("SCAN_PATH_DATA_OFFSET", offset_of!(PathRecord, data_offset)),
        ("SCAN_PATH_BBOX", offset_of!(PathRecord, tile_x0)),
        ("SCAN_PATH_TRANSFORM", offset_of!(PathRecord, transform)),
        ("SCAN_CHUNK_STRIDE", size_of::<GpuScanChunk>()),
        ("SCAN_CHUNK_RANGE_STRIDE", size_of::<GpuScanChunkRange>()),
        ("SCAN_TILE_RANGE_STRIDE", size_of::<TileSegmentRange>()),
        ("SCAN_LINE_STRIDE", size_of::<Line>()),
        ("SCAN_LINE_P0", offset_of!(Line, p0)),
        ("SCAN_LINE_P1", offset_of!(Line, p1)),
        ("SCAN_SEGMENT_STRIDE", size_of::<LineSegment>()),
        ("SCAN_SEGMENT_Y_EDGE", offset_of!(LineSegment, y_edge)),
    ];
    assert_eq!(abi.as_object().unwrap().len(), expected.len());
    for (name, value) in expected {
        assert_eq!(abi[name], value, "{name}");
    }
}
