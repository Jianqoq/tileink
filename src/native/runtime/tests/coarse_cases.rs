use crate::native::runtime::{Result, compute::ComputeBatch};
use crate::shared::gpu_constants::COARSE_WORKGROUP_SIZE;

pub(super) fn prefix_case(count: u32, sparse: bool) -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
    let bytes = |words: &[u32]| {
        words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>()
    };
    let tiles = if sparse { count * 2 + 3 } else { count };
    let chunk_count = count.div_ceil(COARSE_WORKGROUP_SIZE);
    let list_base = tiles * 6 + 5;
    let mut work = vec![0x5a5a5a5a; (list_base + count + 7) as usize];
    let indices: Vec<_> = (0..count)
        .map(|i| if sparse { (count - i) * 2 } else { i })
        .collect();
    for (i, &tile) in indices.iter().enumerate() {
        work[tile as usize * 6] = [0, 1, u32::MAX, 0x80000001, 19][i % 5];
        work[tile as usize * 6 + 3] = [5, u32::MAX, 0, 257, 1][i % 5];
    }
    work[list_base as usize..(list_base + count) as usize].copy_from_slice(&indices);
    let mut expected_work = work.clone();
    let mut expected_chunks = vec![0x5a5a5a5a; (chunk_count as usize + 2) * 4];
    let mut carry = [0u32; 2];
    for (chunk, tiles) in indices.chunks(COARSE_WORKGROUP_SIZE as usize).enumerate() {
        let start = carry;
        for &tile in tiles {
            let base = tile as usize * 6;
            for (kind, value) in carry.iter_mut().enumerate() {
                expected_work[base + kind * 3 + 1] = *value;
                *value = value.wrapping_add(work[base + kind * 3]);
                expected_work[base + kind * 3 + 2] = *value;
            }
        }
        expected_chunks[chunk * 4..chunk * 4 + 4].copy_from_slice(&[
            carry[0].wrapping_sub(start[0]),
            start[0],
            carry[1].wrapping_sub(start[1]),
            start[1],
        ]);
    }
    let mut batch = ComputeBatch::new();
    let mut config = [0; 19];
    config[0] = tiles;
    config[9] = chunk_count;
    config[16] = count;
    config[17] = list_base;
    config[18] = u32::from(sparse);
    let config = batch.buffer(bytes(&config))?;
    let work = batch.buffer(bytes(&work))?;
    let chunks = batch.buffer(bytes(&vec![0x5a5a5a5a; (chunk_count as usize + 2) * 4]))?;
    // SAFETY: unique bounded tile mappings and disjoint metadata/list storage.
    // The three passes fully initialize each logical chunk and retain guard words.
    unsafe {
        batch.dispatch(
            "coarse_prefix_chunks",
            &[(0, config), (7, work), (8, chunks)],
            [chunk_count, 1, 1],
        )?;
        batch.dispatch(
            "coarse_chunk_offsets",
            &[(0, config), (8, chunks)],
            [1, 1, 1],
        )?;
        batch.dispatch(
            "coarse_apply_chunk_offsets",
            &[(0, config), (7, work), (8, chunks)],
            [chunk_count, 1, 1],
        )?;
    }
    batch.readback(work)?;
    batch.readback(chunks)?;
    Ok((batch, vec![bytes(&expected_work), bytes(&expected_chunks)]))
}
