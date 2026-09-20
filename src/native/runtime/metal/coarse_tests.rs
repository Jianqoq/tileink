use super::*;

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn coarse_prefix_matches_cpu_pairs_sparse_tiles_and_multi_block_carries() -> Result<()> {
    let mut device = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for count in [0, 1, 255, 256, 257, 65537] {
        for sparse in [false, true] {
            let (batch, expected) = coarse_cases::prefix_case(count, sparse)?;
            let ticket = device.submit_compute(&batch)?;
            assert_eq!(
                device.readback_batch(&ticket)?,
                expected,
                "count={count} sparse={sparse}"
            );
        }
    }
    device.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn coarse_allocation_keeps_guards_truncation_and_reverse_tile_ranges() -> Result<()> {
    let routes = coarse_routes::Routes::new()?;
    for tiles in [0, 1, 255, 256, 257, 513] {
        for truncate in [false, true] {
            let (batch, expected) = allocation_cases::emit_case(tiles, truncate)?;
            routes.check(
                &batch,
                &expected,
                &format!("allocation tiles={tiles} truncate={truncate}"),
            )?;
        }
        let (batch, expected) = allocation_cases::particle_offset_case(tiles)?;
        routes.check(
            &batch,
            &expected,
            &format!("particle offsets tiles={tiles}"),
        )?;
    }
    routes.validate()
}
