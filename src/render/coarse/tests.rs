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

#[cfg(feature = "metal")]
#[test]
fn large_metal_sparse_coarse_batches_tiles_without_changing_prefix_allocation() {
    use CoarseProgram::*;
    for active in [0, 1, 255, 256, 257, 323] {
        for has_draws in [false, true] {
            let plan = CoarsePlan::new(
                lengths(),
                CoarseBatch {
                    draw_end: if has_draws { 7 } else { 0 },
                    active_tile_count: Some(active),
                    ..Default::default()
                },
                0,
                true,
                65535,
            )
            .unwrap();
            if active == 0 {
                assert!(plan.passes().is_empty());
                continue;
            }
            let scalar = active >= COARSE_WORKGROUP_SIZE;
            let mut expected = vec![
                if scalar { CountBins } else { CountTiles },
                PrefixChunks,
                ChunkOffsets,
                ApplyChunkOffsets,
            ];
            if has_draws {
                expected.push(if scalar { EmitBins } else { EmitTiles });
            }
            assert_eq!(
                programs(&plan),
                expected,
                "active={active} draws={has_draws}"
            );
            assert_eq!(
                plan.passes()[0].grid,
                [
                    if scalar {
                        active.div_ceil(COARSE_WORKGROUP_SIZE)
                    } else {
                        active
                    },
                    1,
                    1
                ]
            );
            assert_eq!(
                plan.config.chunk_count,
                active.div_ceil(COARSE_WORKGROUP_SIZE)
            );
            assert_eq!(plan.config.incremental, 1);
            assert_eq!(plan.config.active_tile_count, active);
        }
    }
}

#[cfg(feature = "metal")]
#[test]
fn preallocated_sparse_slots_preserve_large_scalar_emission_and_skip_empty_draws() {
    use CoarseProgram::*;
    for active in [0, 1, 255, 256, 257, 323] {
        for has_draws in [false, true] {
            let mut plan = CoarsePlan::new(
                lengths(),
                CoarseBatch {
                    draw_end: if has_draws { 7 } else { 0 },
                    active_tile_count: Some(active),
                    ..Default::default()
                },
                0,
                true,
                65535,
            )
            .unwrap();
            plan.use_preallocated_tiles(lengths(), 65535).unwrap();
            if active == 0 || !has_draws {
                assert!(plan.passes().is_empty());
            } else {
                let scalar = active >= COARSE_WORKGROUP_SIZE;
                assert_eq!(programs(&plan), [if scalar { EmitBins } else { EmitTiles }]);
                assert_eq!(
                    plan.passes()[0].grid,
                    [
                        if scalar {
                            active.div_ceil(COARSE_WORKGROUP_SIZE)
                        } else {
                            active
                        },
                        1,
                        1
                    ]
                );
            }
        }
    }
}
