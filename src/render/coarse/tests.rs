use super::*;
fn lengths() -> GpuBufferLengths {
    GpuBufferLengths {
        tiles_width: 17,
        tiles_height: 19,
        tile_count: 323,
        coarse_chunk_count: 2,
        coarse_ptcl_capacity: 900,
        tile_draw_chunk_count: 300,
        ..Default::default()
    }
}
fn batch() -> CoarseBatch {
    CoarseBatch {
        draw_end: 7,
        ..Default::default()
    }
}
fn programs(plan: &CoarsePlan) -> Vec<CoarseProgram> {
    plan.passes().iter().map(|p| p.program).collect()
}
#[test]
fn dense_and_sparse_coarse_preserve_count_prefix_emit_order() {
    use CoarseProgram::*;
    let dense = CoarsePlan::new(lengths(), batch(), 11, false, 65535).unwrap();
    assert_eq!(
        programs(&dense),
        [
            CountBins,
            PrefixChunks,
            ChunkOffsets,
            ApplyChunkOffsets,
            EmitBins
        ]
    );
    assert_eq!(dense.config.paint_brush_base, 11);
    assert_eq!(dense.passes()[0].grid, [4, 1, 1]);
    let sparse = CoarsePlan::new(
        lengths(),
        CoarseBatch {
            active_tile_count: Some(3),
            ..batch()
        },
        0,
        true,
        65535,
    )
    .unwrap();
    assert_eq!(
        programs(&sparse),
        [
            CountTiles,
            PrefixChunks,
            ChunkOffsets,
            ApplyChunkOffsets,
            EmitTiles
        ]
    );
    assert_eq!(sparse.config.chunk_count, 1);
    assert_eq!(sparse.passes()[0].grid, [3, 1, 1]);
}
#[test]
fn chunked_coarse_allocation_precedes_counts_and_emission() {
    use CoarseProgram::*;
    let plan = CoarsePlan::new(lengths(), batch(), 0, true, 18).unwrap();
    assert_eq!(
        programs(&plan),
        [
            EmitChunkCounts,
            EmitPrefixChunks,
            EmitChunkOffsets,
            EmitApplyChunkOffsets,
            EmitFillRefs,
            EmitChunkParticleCounts,
            TileCountsFromEmitChunks,
            PrefixChunks,
            ChunkOffsets,
            ApplyChunkOffsets,
            EmitChunkParticleOffsets,
            EmitChunks,
            EmitChunkTileKinds
        ]
    );
    assert_eq!(plan.passes()[5].grid, [18, 17, 1]);
    assert_eq!(plan.passes()[11].grid, [18, 17, 1]);
    assert!(CoarsePlan::new(lengths(), batch(), 0, true, 17).is_err());
}
#[test]
fn empty_work_skips_dispatch_but_empty_draws_still_clear_tile_counts() {
    assert!(
        CoarsePlan::new(
            GpuBufferLengths::default(),
            CoarseBatch::default(),
            0,
            false,
            1
        )
        .unwrap()
        .passes()
        .is_empty()
    );
    assert!(
        CoarsePlan::new(
            lengths(),
            CoarseBatch {
                active_tile_count: Some(0),
                ..batch()
            },
            0,
            false,
            1
        )
        .unwrap()
        .passes()
        .is_empty()
    );
    let plan = CoarsePlan::new(lengths(), CoarseBatch::default(), 0, true, 65535).unwrap();
    assert_eq!(plan.passes().len(), 4);
    assert_eq!(plan.passes()[0].program, CoarseProgram::CountBins);
}
#[test]
fn invalid_coarse_dimensions_ranges_and_limits_are_rejected() {
    for limit in [0, 65536] {
        assert!(CoarsePlan::new(lengths(), batch(), 0, false, limit).is_err());
    }
    for malformed in [
        GpuBufferLengths {
            tile_count: 324,
            ..lengths()
        },
        GpuBufferLengths {
            coarse_chunk_count: 1,
            ..lengths()
        },
        GpuBufferLengths {
            text_run_count: usize::MAX,
            ..lengths()
        },
    ] {
        assert!(CoarsePlan::new(malformed, batch(), 0, false, 65535).is_err());
    }
    for malformed in [
        CoarseBatch {
            draw_start: 8,
            ..batch()
        },
        CoarseBatch {
            layer_stack_start: 1,
            ..batch()
        },
        CoarseBatch {
            active_tile_count: Some(324),
            ..batch()
        },
    ] {
        assert!(CoarsePlan::new(lengths(), malformed, 0, false, 65535).is_err());
    }
}

#[test]
fn resolving_pipelines_keeps_every_live_dispatch_and_skips_padding() {
    for chunked in [false, true] {
        let plan = CoarsePlan::new(lengths(), batch(), 0, chunked, 65535).unwrap();
        let mut visits = Vec::new();
        let resolved = plan
            .resolve(|pass| {
                visits.push(pass.program);
                pass.program
            })
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        assert_eq!(resolved, programs(&plan));
        assert_eq!(visits, resolved);
    }
}
