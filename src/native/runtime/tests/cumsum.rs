use super::*;
#[test]
fn cumsum_metadata_rejects_overlapping_writes_missing_rows_and_long_chunks() {
    for (offsets, lengths, starts, ends, size) in [
        (vec![0], vec![257], vec![0], vec![1], 257),
        (vec![0, 1], vec![2, 1], vec![0], vec![2], 3),
        (vec![0], vec![2], vec![0], vec![1], 1),
        (vec![0], vec![1], vec![], vec![], 1),
        (vec![0, 1], vec![1, 1], vec![0, 0], vec![1, 2], 2),
        (vec![0], vec![1], vec![0], vec![0], 1),
    ] {
        assert!(CumsumPlan::new(offsets, lengths, starts, ends, size).is_err());
    }
    assert!(CumsumPlan::new(vec![], vec![], vec![], vec![], 0).is_ok());
}
#[test]
fn cumsum_encodes_ordered_stages_and_guards_padded_two_dimensional_groups() {
    let plan = CumsumPlan::new(vec![0, 1, 2, 3, 4], vec![1; 5], vec![0], vec![5], 5).unwrap();
    let mut batch = ComputeBatch::new();
    let backdrops = batch.buffer(vec![0; 20]).unwrap();
    let output = plan.encode(&mut batch, backdrops, 3).unwrap().unwrap();
    assert_eq!(
        batch
            .passes()
            .iter()
            .map(|p| p.shader.entry)
            .collect::<Vec<_>>(),
        [
            "cumsum_prefix_chunks",
            "cumsum_chunk_offsets",
            "cumsum_apply_chunk_offsets"
        ]
    );
    assert_eq!(batch.passes()[0].grid, [3, 2, 1]);
    assert_eq!(batch.size(output.totals).unwrap(), 20);
    assert_eq!(batch.size(output.offsets).unwrap(), 20);
    let plan = CumsumPlan::new(vec![0, 1], vec![1; 2], vec![0, 1], vec![1, 2], 2).unwrap();
    let mut batch = ComputeBatch::new();
    let backdrops = batch.buffer(vec![0; 8]).unwrap();
    plan.encode(&mut batch, backdrops, 65535).unwrap();
    assert_eq!(batch.passes().len(), 1);
}

#[test]
fn cumsum_accepts_empty_arena_rows_and_unowned_empty_chunks() {
    let plan = CumsumPlan::new(
        vec![0, 0, 1, 0],
        vec![0, 1, 1, 0],
        vec![0, 1, 0, 0],
        vec![0, 3, 0, 0],
        2,
    )
    .unwrap();
    let mut batch = ComputeBatch::new();
    let backdrops = batch.buffer(vec![0; 8]).unwrap();
    plan.encode(&mut batch, backdrops, 65535).unwrap();
    assert_eq!(
        batch.passes().len(),
        3,
        "equal table lengths do not imply one chunk per live row"
    );
    let empty = CumsumPlan::new(vec![0], vec![0], vec![], vec![], 0).unwrap();
    let mut batch = ComputeBatch::new();
    let backdrops = batch.buffer(vec![0; 4]).unwrap();
    empty.encode(&mut batch, backdrops, 65535).unwrap();
    assert_eq!(batch.passes().len(), 1);
}
