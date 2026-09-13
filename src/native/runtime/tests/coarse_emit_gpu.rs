use super::reference;
use crate::native::{
    NativeBackend,
    runtime::{Result, adapter::Adapter, compute::ComputeBatch},
};
use crate::shared::gpu_constants::COARSE_WORKGROUP_SIZE;

fn emit_case(tiles: u32, truncate: bool) -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
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

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_coarse_emit_allocation_preserves_capacity_and_record_guards() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    for tiles in [0, 1, 255, 256, 257, 513] {
        for truncate in [false, true] {
            let (batch, expected) = emit_case(tiles, truncate)?;
            for repetition in 0..3 {
                let mut outputs = Vec::new();
                for reference in &references {
                    outputs.push(reference.execute_compute(&batch)?);
                }
                for device in [&dx12, &vulkan] {
                    outputs.push(
                        device
                            .submit_compute(&batch)
                            .map_err(|e| format!("{e:?}"))?
                            .readback()?,
                    );
                }
                for (route, actual) in outputs.iter().enumerate() {
                    assert_eq!(actual.len(), expected.len());
                    for (buffer, (a, b)) in actual.iter().zip(&expected).enumerate() {
                        let first = a.iter().zip(b).position(|(a, b)| a != b);
                        assert!(
                            a.len() == b.len() && first.is_none(),
                            "tiles {tiles} truncate {truncate} repetition {repetition} route {route} buffer {buffer} first {first:?}"
                        );
                    }
                }
            }
        }
    }
    dx12.assert_valid()?;
    vulkan.assert_valid()?;
    Ok(())
}
