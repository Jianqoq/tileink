//! Shared scan inputs and independent CPU prefix oracle for native API acceptance.
use crate::native::runtime::{Result, compute::ComputeBatch};
use crate::shared::{
    gpu_plan::{GpuScanChunk, GpuScanChunkRange},
    line::Line,
    path::PathRecord,
};
pub(crate) fn prefix_case(lengths: &[u32], seed: u32) -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
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

pub(crate) fn count_case(p0: [f32; 2], p1: [f32; 2]) -> Result<ComputeBatch> {
    let words = |values: &[u32]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(words(&[0, 64, 1, 0, 1, 0, 0, 0, 0, 0, 0]))?;
    let lines = batch.buffer(
        bytemuck::bytes_of(&Line {
            path_id: 0,
            _pad: 0.0,
            p0,
            p1,
        })
        .to_vec(),
    )?;
    let paths = batch.buffer(
        bytemuck::bytes_of(&PathRecord {
            path_id: 0,
            line_count: 1,
            data_offset: 7,
            data_len: 64,
            tile_x1: 8,
            tile_y1: 8,
            ..Default::default()
        })
        .to_vec(),
    )?;
    let mut initial = vec![0x12345678; 7];
    initial.extend([0; 64]);
    initial.extend([0x12345678; 7]);
    let backdrops = batch.buffer(words(&initial))?;
    let counts = batch.buffer(words(&initial))?;
    let active = batch.buffer(words(&[0]))?;
    // SAFETY: the finite, bounded test segment and 8x8 path own all 64 tile
    // records; guards surround both destinations and the sole path/line exist.
    unsafe {
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
    }
    batch.readback(backdrops)?;
    batch.readback(counts)?;
    Ok(batch)
}
