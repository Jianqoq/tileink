use super::*;
fn setup() -> (GpuBufferLengths, FineParams) {
    (
        GpuBufferLengths {
            tile_count: 256,
            tiles_width: 16,
            tiles_height: 16,
            ..Default::default()
        },
        FineParams {
            width: TILE_SIZE * 16,
            height: TILE_SIZE * 16,
            ..Default::default()
        },
    )
}
#[test]
fn fine_grid_keeps_linear_order_at_two_dimensional_limit() {
    let (lengths, params) = setup();
    assert_eq!(
        FinePlan::new(lengths, params, 16).unwrap().grid(),
        [16, 16, 1]
    );
    assert!(FinePlan::new(lengths, params, 15).is_err());
    let sparse = FinePlan::new(
        lengths,
        FineParams {
            active_tile_count: Some(17),
            ..params
        },
        16,
    )
    .unwrap();
    assert_eq!(sparse.grid(), [16, 2, 1]);
    assert_eq!(sparse.config().incremental, 1);
    assert_eq!(sparse.config().active_tile_count, 17);
}
#[test]
fn fine_empty_and_invalid_inputs_do_not_launch_work() {
    let (lengths, params) = setup();
    assert_eq!(
        FinePlan::new(GpuBufferLengths::default(), FineParams::default(), 1)
            .unwrap()
            .grid(),
        [0; 3]
    );
    assert_eq!(
        FinePlan::new(
            lengths,
            FineParams {
                active_tile_count: Some(0),
                ..params
            },
            1
        )
        .unwrap()
        .grid(),
        [0; 3]
    );
    assert!(FinePlan::new(lengths, FineParams { width: 0, ..params }, 16).is_err());
    assert!(
        FinePlan::new(
            lengths,
            FineParams {
                active_tile_count: Some(257),
                ..params
            },
            16
        )
        .is_err()
    );
    for limit in [0, 65536] {
        assert!(FinePlan::new(lengths, params, limit).is_err());
    }
}
#[test]
fn fine_spills_partition_every_physical_lane_including_sparse_frames() {
    let (lengths, params) = setup();
    let plan = FinePlan::new(
        lengths,
        FineParams {
            clip_spill_depth: 3,
            group_spill_depth: 2,
            active_tile_count: Some(1),
            ..params
        },
        16,
    )
    .unwrap();
    let lanes = lengths.tile_count * FINE_WORKGROUP_SIZE as usize;
    assert_eq!(plan.config().group_spill_base as usize, lanes * 3);
    assert_eq!(
        plan.spill_words(),
        lanes * (3 + 2 * FINE_GROUP_SPILL_FIELDS as usize)
    );
    assert!(spill_layout(u32::MAX, u32::MAX, u32::MAX).is_err());
    assert_eq!(spill_layout(1, 0, 0).unwrap(), (0, 0));
    let largest = u32::MAX / 4 / FINE_WORKGROUP_SIZE;
    assert!(spill_layout(largest, 1, 0).is_ok());
    assert!(spill_layout(largest + 1, 1, 0).is_err());
}
#[test]
fn fine_rejects_unrepresentable_coarse_layouts_before_offset_arithmetic() {
    let (lengths, params) = setup();
    for malformed in [
        GpuBufferLengths {
            coarse_glyph_capacity: usize::MAX,
            ..lengths
        },
        GpuBufferLengths {
            tile_draw_index_count: usize::MAX,
            ..lengths
        },
        GpuBufferLengths {
            tile_draw_chunk_count: usize::MAX,
            ..lengths
        },
    ] {
        assert!(FinePlan::new(malformed, params, 16).is_err());
    }
}
