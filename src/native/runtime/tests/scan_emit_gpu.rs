//! Full ordered scan chain: no CPU readback occurs between the six kernels.
use super::{Result, reference};
use crate::{
    NativeBackend,
    native::runtime::{adapter::Adapter, compute::ComputeBatch},
    shared::{
        gpu_plan::{GpuScanChunk, GpuScanChunkRange},
        line::Line,
        path::PathRecord,
    },
};

fn emit_case(p0: [f32; 2], p1: [f32; 2]) -> Result<ComputeBatch> {
    let words = |values: &[u32]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let mut batch = ComputeBatch::new();
    // Sparse index lists let the clear pass preserve both surrounding guards.
    let config = batch.buffer(words(&[64, 64, 1, 1, 1, 263, 1, 0, 1, 2, 3]))?;
    let mut indices = vec![0, 0, 0];
    indices.extend(7..71);
    let active = batch.buffer(words(&indices))?;
    let lines = batch.buffer(
        bytemuck::bytes_of(&Line {
            path_id: 0,
            _pad: 0.0,
            p0,
            p1,
        })
        .to_vec(),
    )?;
    let path = PathRecord {
        path_id: 0,
        line_count: 1,
        data_offset: 7,
        data_len: 64,
        tile_x1: 8,
        tile_y1: 8,
        segment_start: 7,
        segment_capacity: 256,
        ..Default::default()
    };
    let paths = batch.buffer(bytemuck::bytes_of(&path).to_vec())?;
    let chunks = batch.buffer(
        bytemuck::bytes_of(&GpuScanChunk {
            path_id: 0,
            backdrop_offset: 7,
            segment_start: 7,
            len: 64,
        })
        .to_vec(),
    )?;
    let chunk_ranges =
        batch.buffer(bytemuck::bytes_of(&GpuScanChunkRange { start: 0, end: 1 }).to_vec())?;
    let backdrops = batch.buffer(words(&[0x12345678; 78]))?;
    let ranges = batch.buffer(words(&[0x12345678; 156]))?;
    let counts = batch.buffer(words(&[0x12345678; 78]))?;
    let cursors = batch.buffer(words(&[0x12345678; 78]))?;
    let bumps = batch.buffer(words(&[0x12345678]))?;
    let totals = batch.buffer(words(&[0x12345678]))?;
    let offsets = batch.buffer(words(&[0x12345678]))?;
    let segments = batch.buffer(words(&[0x12345678; 270 * 5]))?;
    // SAFETY: one finite line/path, one 64-word chunk, disjoint bounded active
    // indices and 256 segment slots. Six ordered stages initialize every value
    // that a later stage reads; emit has its own capacity check before storage.
    unsafe {
        batch.dispatch(
            "scan_clear",
            &[
                (0, config),
                (1, backdrops),
                (2, ranges),
                (3, counts),
                (4, cursors),
                (5, bumps),
                (6, totals),
                (7, offsets),
                (8, active),
            ],
            [1, 1, 1],
        )?;
        batch.dispatch(
            "scan_count",
            &[
                (0, config),
                (1, lines),
                (2, paths),
                (3, backdrops),
                (4, counts),
                (5, active),
            ],
            [1, 1, 1],
        )?;
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
            [1, 1, 1],
        )?;
        batch.dispatch(
            "scan_chunk_offsets",
            &[
                (0, config),
                (1, paths),
                (2, chunk_ranges),
                (3, bumps),
                (4, totals),
                (5, offsets),
                (6, active),
            ],
            [1, 1, 1],
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
            [1, 1, 1],
        )?;
        batch.dispatch(
            "scan_emit",
            &[
                (0, config),
                (1, lines),
                (2, paths),
                (3, cursors),
                (4, segments),
                (5, active),
            ],
            [1, 1, 1],
        )?;
    }
    for id in [
        backdrops, ranges, counts, cursors, bumps, totals, offsets, segments,
    ] {
        batch.readback(id)?;
    }
    Ok(batch)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_scan_chain_matches_clipped_segments_bit_for_bit() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    let mut cases = vec![
        ([8.0, 0.0], [8.0, 48.0]),
        ([0.0, 8.0], [48.0, 8.0]),
        ([-8.0, 0.0], [-8.0, 48.0]),
        ([0.0, 0.0], [48.0, 48.0]),
        ([16.0, 16.0], [16.0, 16.0]),
        ([0.0, 16.0], [128.0, 16.0]),
        ([-16.0, -16.0], [144.0, 128.0]),
        ([0.0, 0.000001], [128.0, 0.000001]),
    ];
    let mut state = 321u32;
    for _ in 0..24 {
        let mut point = || {
            let mut p = [0.0; 2];
            for v in &mut p {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                *v = (state % 3073) as f32 / 16.0 - 32.0;
            }
            p
        };
        cases.push((point(), point()));
    }
    for repetition in 0..3 {
        for &(a, b) in &cases {
            for (p0, p1) in [(a, b), (b, a)] {
                let batch = emit_case(p0, p1)?;
                let expected = references[0].execute_compute(&batch)?;
                assert_eq!(
                    references[1].execute_compute(&batch)?,
                    expected,
                    "wgpu {p0:?} {p1:?}"
                );
                for (route, device) in [("DX12", &dx12), ("Vulkan", &vulkan)] {
                    let receipt = device
                        .submit_compute(&batch)
                        .map_err(|e| format!("{e:?}"))?;
                    let actual = receipt.readback()?;
                    assert_eq!(actual.len(), expected.len());
                    for (buffer, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
                        let first = actual.iter().zip(expected).position(|(a, b)| a != b);
                        assert!(
                            actual.len() == expected.len() && first.is_none(),
                            "{route} {p0:?} {p1:?} repeat {repetition} buffer {buffer} first byte {first:?}"
                        );
                    }
                }
                let bytes = &expected[7];
                for word in bytes[..7 * 20]
                    .chunks_exact(4)
                    .chain(bytes[263 * 20..].chunks_exact(4))
                {
                    assert_eq!(u32::from_le_bytes(word.try_into().unwrap()), 0x12345678);
                }
            }
        }
    }
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    Ok(())
}

// Each path owns a disjoint tile/segment interval, so atomic allocation has one
// writer per tile. This verifies record strides and transforms without making
// intermediate segment order depend on scheduling of overlapping lines.
fn affine_chain_case(count: u32) -> Result<ComputeBatch> {
    use crate::shared::affine::GpuAffine;
    use crate::shared::gpu_constants::SCAN_CHUNK_SIZE;
    let words = |values: &[u32]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let mut batch = ComputeBatch::new();
    let guard = 7;
    let tile_end = guard + count * 64;
    let segment_end = guard + count * 32;
    let config = batch.buffer(words(&[
        count * 64,
        count * 64,
        count,
        count,
        count,
        segment_end,
        1,
        0,
        count,
        count * 2,
        count * 3,
    ]))?;
    let mut active = Vec::new();
    // Reverse mappings exercise every nonzero record stride and sparse lookup.
    for _ in 0..3 {
        active.extend((0..count).rev());
    }
    active.extend(guard..tile_end);
    let active = batch.buffer(words(&active))?;
    let mut lines = Vec::new();
    let mut paths = Vec::new();
    let mut chunks = Vec::new();
    let mut chunk_ranges = Vec::new();
    for index in 0..count {
        let transform = match index % 4 {
            0 => GpuAffine {
                a: 0.75,
                b: 0.125,
                c: -0.25,
                d: 1.25,
                e: 17.5,
                f: -3.25,
            },
            1 => GpuAffine {
                a: 0.0,
                b: 1.0,
                c: -1.0,
                d: 0.0,
                e: 112.0,
                f: 4.5,
            },
            2 => GpuAffine {
                a: -0.5,
                b: -0.25,
                c: 0.125,
                d: 0.875,
                e: 80.0,
                f: 24.0,
            },
            _ => GpuAffine {
                a: 1.25,
                b: -0.125,
                c: 0.25,
                d: 0.5,
                e: -16.0,
                f: 42.0,
            },
        };
        let t = (index % 17) as f32 / 16.0;
        let (p0, p1) = ([4.25 + t, 8.5 - t], [100.75 - t, 104.25 + t]);
        let (p0, p1) = if index % 2 == 0 { (p0, p1) } else { (p1, p0) };
        lines.push(Line {
            path_id: index,
            _pad: 0.0,
            p0,
            p1,
        });
        paths.push(PathRecord {
            path_id: index,
            line_start: index,
            line_count: 1,
            data_offset: guard + index * 64,
            data_len: 64,
            tile_x1: 8,
            tile_y1: 8,
            segment_start: guard + index * 32,
            segment_capacity: 32,
            transform,
            ..Default::default()
        });
        chunks.push(GpuScanChunk {
            path_id: index,
            backdrop_offset: guard + index * 64,
            segment_start: guard + index * 32,
            len: 64,
        });
        chunk_ranges.push(GpuScanChunkRange {
            start: index,
            end: index + 1,
        });
    }
    let lines = batch.buffer(bytemuck::cast_slice(&lines).to_vec())?;
    let paths = batch.buffer(bytemuck::cast_slice(&paths).to_vec())?;
    let chunks = batch.buffer(bytemuck::cast_slice(&chunks).to_vec())?;
    let chunk_ranges = batch.buffer(bytemuck::cast_slice(&chunk_ranges).to_vec())?;
    let mut allocate = |size: u32| batch.buffer(words(&vec![0x12345678; size as usize]));
    let backdrops = allocate(tile_end + guard)?;
    let ranges = allocate((tile_end + guard) * 2)?;
    let counts = allocate(tile_end + guard)?;
    let cursors = allocate(tile_end + guard)?;
    let bumps = allocate(count)?;
    let totals = allocate(count)?;
    let offsets = allocate(count)?;
    let segments = allocate((segment_end + guard) * 5)?;
    let threads = [count.div_ceil(SCAN_CHUNK_SIZE), 1, 1];
    let chunk_grid = [count.min(3), count.div_ceil(3), 1];
    // SAFETY: all mappings are bijective over bounded records. Each 64-tile path
    // has one finite line crossing at most 16 tiles and 32 reserved segments.
    // The clear pass initializes every logical read; padded groups are guarded.
    unsafe {
        batch.dispatch(
            "scan_clear",
            &[
                (0, config),
                (1, backdrops),
                (2, ranges),
                (3, counts),
                (4, cursors),
                (5, bumps),
                (6, totals),
                (7, offsets),
                (8, active),
            ],
            [(count * 64).div_ceil(SCAN_CHUNK_SIZE), 1, 1],
        )?;
        batch.dispatch(
            "scan_count",
            &[
                (0, config),
                (1, lines),
                (2, paths),
                (3, backdrops),
                (4, counts),
                (5, active),
            ],
            threads,
        )?;
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
            chunk_grid,
        )?;
        batch.dispatch(
            "scan_chunk_offsets",
            &[
                (0, config),
                (1, paths),
                (2, chunk_ranges),
                (3, bumps),
                (4, totals),
                (5, offsets),
                (6, active),
            ],
            threads,
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
            chunk_grid,
        )?;
        batch.dispatch(
            "scan_emit",
            &[
                (0, config),
                (1, lines),
                (2, paths),
                (3, cursors),
                (4, segments),
                (5, active),
            ],
            threads,
        )?;
    }
    for id in [
        backdrops, ranges, counts, cursors, bumps, totals, offsets, segments,
    ] {
        batch.readback(id)?;
    }
    Ok(batch)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_scan_chain_matches_affine_paths_across_workgroups() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    for count in [17, 255, 256, 257] {
        let batch = affine_chain_case(count)?;
        let expected = references[0].execute_compute(&batch)?;
        for repetition in 0..3 {
            assert_eq!(
                references[1].execute_compute(&batch)?,
                expected,
                "wgpu affine count {count}"
            );
            for device in [&dx12, &vulkan] {
                let actual = device
                    .submit_compute(&batch)
                    .map_err(|e| format!("{e:?}"))?
                    .readback()?;
                assert_eq!(actual.len(), expected.len());
                for (index, (a, b)) in actual.iter().zip(&expected).enumerate() {
                    let first = a.iter().zip(b).position(|(a, b)| a != b);
                    assert!(
                        a.len() == b.len() && first.is_none(),
                        "affine count {count} repetition {repetition} buffer {index} first byte {first:?}"
                    );
                }
            }
        }
        for word in expected[7][..7 * 20]
            .chunks_exact(4)
            .chain(expected[7][(7 + count as usize * 32) * 20..].chunks_exact(4))
        {
            assert_eq!(u32::from_le_bytes(word.try_into().unwrap()), 0x12345678);
        }
    }
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    Ok(())
}
