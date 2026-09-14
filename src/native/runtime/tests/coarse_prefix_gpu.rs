use super::reference;
use crate::native::{
    NativeBackend,
    runtime::{Result, adapter::Adapter, compute::ComputeBatch},
};
use crate::shared::gpu_constants::COARSE_WORKGROUP_SIZE;

fn prefix_case(count: u32, sparse: bool) -> Result<(ComputeBatch, Vec<Vec<u8>>)> {
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

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_coarse_prefix_matches_cpu_pairs_and_sparse_guards() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    let dx12 = Adapter::new(NativeBackend::Dx12, &identity)?;
    let vulkan = Adapter::new(NativeBackend::Vulkan, &identity)?;
    let references = [
        reference::Reference::new(wgpu::Backends::DX12, &identity)?,
        reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
    ];
    // 65537 items force the chunk-offset scan to carry across its own block.
    for count in [0, 1, 255, 256, 257, 65537] {
        for sparse in [false, true] {
            let (batch, expected) = prefix_case(count, sparse)?;
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
                            "coarse count {count} sparse {sparse} repetition {repetition} route {route} buffer {buffer} first byte {first:?}"
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

#[test]
fn coarse_raw_record_abi_matches_host_layout() {
    use crate::shared::gpu_coarse::{
        CoarseChunkRecord, EmitChunkRecord, PtclRecord, TileCoarseRecord, TileDrawRecord,
        TileEmitChunkRecord,
    };
    use std::mem::{offset_of, size_of};
    let abi =
        super::hlsl_constants::parse(include_str!("../../../shaders/hlsl/coarse_records.hlsli"))
            .unwrap();
    let expected = [
        (
            "COARSE_EMIT_PTCL_COUNT",
            offset_of!(EmitChunkRecord, ptcl_count),
        ),
        (
            "COARSE_EMIT_PTCL_OFFSET",
            offset_of!(EmitChunkRecord, ptcl_offset),
        ),
        (
            "COARSE_EMIT_GLYPH_COUNT",
            offset_of!(EmitChunkRecord, glyph_count),
        ),
        (
            "COARSE_EMIT_GLYPH_OFFSET",
            offset_of!(EmitChunkRecord, glyph_offset),
        ),
        ("COARSE_PTCL_RECORD_STRIDE", size_of::<PtclRecord>()),
        (
            "COARSE_TILE_DRAW_RECORD_STRIDE",
            size_of::<TileDrawRecord>(),
        ),
        (
            "COARSE_TILE_EMIT_RECORD_STRIDE",
            size_of::<TileEmitChunkRecord>(),
        ),
        ("COARSE_EMIT_RECORD_STRIDE", size_of::<EmitChunkRecord>()),
        ("COARSE_TILE_DRAW_COUNT", offset_of!(TileDrawRecord, end)),
        (
            "COARSE_EMIT_CLASS_FLAGS",
            offset_of!(EmitChunkRecord, class_flags),
        ),
        ("COARSE_TILE_RECORD_STRIDE", size_of::<TileCoarseRecord>()),
        (
            "COARSE_TILE_GLYPH_COUNT",
            offset_of!(TileCoarseRecord, glyph_count),
        ),
        (
            "COARSE_TILE_PTCL_START",
            offset_of!(TileCoarseRecord, ptcl_start),
        ),
        (
            "COARSE_TILE_GLYPH_START",
            offset_of!(TileCoarseRecord, glyph_start),
        ),
        ("COARSE_CHUNK_RECORD_STRIDE", size_of::<CoarseChunkRecord>()),
        (
            "COARSE_CHUNK_GLYPH_TOTAL",
            offset_of!(CoarseChunkRecord, glyph_total),
        ),
        (
            "COARSE_CHUNK_PTCL_OFFSET",
            offset_of!(CoarseChunkRecord, ptcl_offset),
        ),
        (
            "COARSE_CHUNK_GLYPH_OFFSET",
            offset_of!(CoarseChunkRecord, glyph_offset),
        ),
    ];
    assert_eq!(abi.len(), expected.len());
    for (name, value) in expected {
        assert_eq!(abi[name] as usize, value, "{name}");
    }
}

#[test]
fn draw_raw_record_abi_matches_host_layout() {
    use crate::shared::{
        draw_record::DrawRecord,
        gpu_coarse::LayerStackRecord,
        gpu_text::{GlyphImageRecord, GlyphRecord, GlyphRunRecord},
    };
    use std::mem::{offset_of, size_of};
    // Check the actual host records: raw HLSL loads must not drift with Rust layout changes.
    let constants =
        super::hlsl_constants::parse(include_str!("../../../shaders/hlsl/draw_records.hlsli"))
            .unwrap();
    let expected = [
        ("DRAW_RECORD_STRIDE", size_of::<DrawRecord>()),
        ("DRAW_PATH", offset_of!(DrawRecord, path_id)),
        ("DRAW_GLYPH_RUN", offset_of!(DrawRecord, glyph_run_id)),
        ("DRAW_SDF", offset_of!(DrawRecord, sdf_offset)),
        ("DRAW_SDF_LEN", offset_of!(DrawRecord, sdf_len)),
        ("DRAW_SHADOW", offset_of!(DrawRecord, sdf_shadow_offset)),
        ("DRAW_BRUSH_OFFSET", offset_of!(DrawRecord, brush_offset)),
        ("DRAW_SOLID_RECT", offset_of!(DrawRecord, solid_rect)),
        ("DRAW_TAG", offset_of!(DrawRecord, tag)),
        ("DRAW_FILL_RULE", offset_of!(DrawRecord, fill_rule)),
        ("DRAW_PIXEL_BOUNDS", offset_of!(DrawRecord, pixel_bounds)),
        ("DRAW_TRANSFORM", offset_of!(DrawRecord, transform)),
        (
            "DRAW_INVERSE_TRANSFORM",
            offset_of!(DrawRecord, inverse_transform),
        ),
        ("LAYER_RECORD_STRIDE", size_of::<LayerStackRecord>()),
        ("GLYPH_RUN_STRIDE", size_of::<GlyphRunRecord>()),
        ("GLYPH_RECORD_STRIDE", size_of::<GlyphRecord>()),
        ("GLYPH_IMAGE_STRIDE", size_of::<GlyphImageRecord>()),
    ];
    assert_eq!(constants.len(), expected.len());
    for (name, value) in expected {
        assert_eq!(constants[name] as usize, value, "{name}");
    }
}
