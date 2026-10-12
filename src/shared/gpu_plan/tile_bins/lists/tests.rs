use super::*;

#[test]
fn exact_build_growing_lists_and_reused_holes_preserve_unrelated_lists() {
    let mut lists = PackedLists::default();
    lists.reset_counts([3, 2, 0].into_iter());
    for value in 0..3 {
        lists.push(0, value);
    }
    lists.push(1, 8);
    lists.push(1, 9);
    assert_eq!(lists.values.len(), 5);
    let fixed = lists.start(1);
    lists.push(0, 3);
    assert_eq!(&lists[0], [0, 1, 2, 3]);
    assert_eq!(&lists[1], [8, 9]);
    assert_eq!(lists.start(1), fixed);
    let end = lists.values.len();
    lists.write(2, &[42, 43]);
    assert_eq!(
        lists.values.len(),
        end,
        "reuse the released exact three-word span"
    );
    assert_eq!(&lists[2], [42, 43]);
}

#[test]
fn stable_retention_empty_reuse_and_fragmented_compaction_preserve_order() {
    let mut lists = PackedLists::default();
    lists.clear(20);
    for tile in 0..20 {
        lists.append_exact(tile, &[tile as u32, 100 + tile as u32]);
    }
    lists.retain(19, |id| id >= 100);
    assert_eq!(&lists[19], [119]);
    for tile in 0..8 {
        lists.retain(tile, |_| false);
        lists.release_empty(tile);
    }
    lists.write(18, &[8, 7, 6, 5, 4]);
    assert!(lists.maybe_compact());
    assert_eq!(lists.values.len(), 26);
    for tile in 0..8 {
        assert!(lists[tile].is_empty());
    }
    for tile in 8..18 {
        assert_eq!(&lists[tile], [tile as u32, 100 + tile as u32]);
    }
    assert_eq!(&lists[18], [8, 7, 6, 5, 4]);
    assert_eq!(&lists[19], [119]);
}

#[test]
fn repeated_edits_match_independent_vectors_across_size_classes() {
    let mut lists = PackedLists::default();
    lists.clear(23);
    let mut reference = vec![Vec::new(); 23];
    let mut random = 17u32;
    for step in 0..4000 {
        random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let tile = random as usize % reference.len();
        let len = ((random >> 11) % 520) as usize;
        reference[tile] = (0..len).map(|i| step + i as u32).collect();
        lists.write(tile, &reference[tile]);
        lists.release_empty(tile);
        lists.maybe_compact();
        for (tile, expected) in reference.iter().enumerate() {
            assert_eq!(&lists[tile], expected);
        }
    }
}
