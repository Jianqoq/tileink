use crate::native::runtime::{Result, compute::ComputeBatch};
use crate::shared::gpu_constants::COARSE_WORKGROUP_SIZE;

pub(super) fn emit_case(tiles: u32, truncate: bool) -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
    let bytes = |words: &[u32]| {
        words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let draws: Vec<u32> = (0..tiles)
        .map(|i| [0, 1, 255, 256, 257, 513][i as usize % 6])
        .collect();
    let counts: Vec<_> = draws
        .iter()
        .map(|count| count.div_ceil(COARSE_WORKGROUP_SIZE))
        .collect();
    let total = counts.iter().sum::<u32>();
    let capacity = if truncate { total / 2 } else { total + 3 };
    let chunks = tiles.div_ceil(COARSE_WORKGROUP_SIZE);
    // Nonzero unrelated capacities exercise every preceding region in the packed ABI.
    let draw_base = tiles * 6 + 13 * 6 + 11;
    let tile_emit_base = draw_base + tiles * 2 + 17;
    let emit_base = tile_emit_base + tiles * 2;
    let mut work = vec![0x42424242; (emit_base + capacity * 7 + tiles + 7) as usize];
    for (tile, &count) in draws.iter().enumerate() {
        work[draw_base as usize + tile * 2 + 1] = count;
    }
    let mut expected = work.clone();
    let mut expected_chunks = vec![0x42424242; (chunks as usize + 2) * 4];
    let mut offset = 0;
    for (tile, &count) in counts.iter().enumerate() {
        expected[tile_emit_base as usize + tile * 2] = count;
        expected[tile_emit_base as usize + tile * 2 + 1] = offset;
        for local in 0..count {
            let index = offset + local;
            if index < capacity {
                let base = (emit_base + index * 7) as usize;
                expected[base] = tile as u32;
                expected[base + 1] = local;
                expected[base + 6] = 0;
            }
        }
        offset += count;
    }
    let mut offset = 0;
    for (chunk, counts) in counts.chunks(COARSE_WORKGROUP_SIZE as usize).enumerate() {
        let total = counts.iter().sum::<u32>();
        expected_chunks[chunk * 4] = total;
        expected_chunks[chunk * 4 + 1] = offset;
        offset += total;
    }
    let mut batch = ComputeBatch::new();
    let mut config = [0; 19];
    config[0] = tiles;
    config[7] = 13;
    config[8] = 11;
    config[9] = chunks;
    config[12] = 17;
    config[13] = capacity;
    let config = batch.buffer(bytes(&config))?;
    let work = batch.buffer(bytes(&work))?;
    let chunk_records = batch.buffer(bytes(&vec![0x42424242; (chunks as usize + 2) * 4]))?;
    // SAFETY: packed regions and all logical records fit; each tile owns disjoint
    // references. Fill refs is explicitly capacity-bounded, including truncation.
    unsafe {
        batch.dispatch(
            "coarse_emit_chunk_counts",
            &[(0, config), (7, work)],
            [chunks, 1, 1],
        )?;
        batch.dispatch(
            "coarse_emit_prefix_chunks",
            &[(0, config), (7, work), (8, chunk_records)],
            [chunks, 1, 1],
        )?;
        batch.dispatch(
            "coarse_emit_chunk_offsets",
            &[(0, config), (8, chunk_records)],
            [1, 1, 1],
        )?;
        batch.dispatch(
            "coarse_emit_apply_chunk_offsets",
            &[(0, config), (7, work), (8, chunk_records)],
            [chunks, 1, 1],
        )?;
        batch.dispatch(
            "coarse_emit_fill_refs",
            &[(0, config), (7, work)],
            [chunks, 1, 1],
        )?;
    }
    batch.readback(work)?;
    batch.readback(chunk_records)?;
    Ok((batch, vec![bytes(&expected), bytes(&expected_chunks)]))
}

pub(super) fn particle_offset_case(tiles: u32) -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
    let bytes = |words: &[u32]| {
        words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let counts: Vec<u32> = (0..tiles)
        .map(|tile| [0, 1, 2, 255, 256, 257][tile as usize % 6])
        .collect();
    let total: u32 = counts.iter().sum();
    let tile_base = tiles * 8 + 13 * 6 + 11 + 17;
    let emit_base = tile_base + tiles * 2;
    let mut work = vec![0x43434343; (emit_base + (total + 3) * 7 + tiles + 7) as usize];
    // Reverse physical chunk ranges to prove offsets reset per tile and follow
    // each tile's explicit range rather than incidental record allocation order.
    let mut end = total;
    for (tile, &count) in counts.iter().enumerate() {
        end -= count;
        work[tile_base as usize + tile * 2] = count;
        work[tile_base as usize + tile * 2 + 1] = end;
        for local in 0..count {
            let base = (emit_base + (end + local) * 7) as usize;
            work[base + 2] = [0, 1, u32::MAX, 0x80000001, 257][local as usize % 5];
            work[base + 4] = [u32::MAX, 3, 0, 7][local as usize % 4];
        }
    }
    let mut expected = work.clone();
    for tile in 0..tiles {
        let count = work[(tile_base + tile * 2) as usize];
        let start = work[(tile_base + tile * 2 + 1) as usize];
        let mut carry = [0u32; 2];
        for index in start..start + count {
            let base = (emit_base + index * 7) as usize;
            for kind in 0..2 {
                expected[base + 3 + kind * 2] = carry[kind];
                carry[kind] = carry[kind].wrapping_add(work[base + 2 + kind * 2]);
            }
        }
    }
    let mut config = [0; 19];
    config[0] = tiles;
    config[7] = 13;
    config[8] = 11;
    config[12] = 17;
    config[13] = total + 3;
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(bytes(&config))?;
    let work = batch.buffer(bytes(&work))?;
    // SAFETY: nonoverlapping per-tile ranges fit in the packed work buffer.
    // One extra workgroup deliberately exercises the logical tile-count guard.
    unsafe {
        batch.dispatch(
            "coarse_emit_chunk_particle_offsets",
            &[(0, config), (7, work)],
            [tiles.div_ceil(COARSE_WORKGROUP_SIZE) + 1, 1, 1],
        )?;
    }
    batch.readback(work)?;
    Ok((batch, vec![bytes(&expected)]))
}
