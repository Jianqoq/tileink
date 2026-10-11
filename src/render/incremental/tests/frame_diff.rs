use super::*;
use std::collections::HashMap;

#[test]
fn revision_change_dirties_union_of_old_and_new_tiles() {
    let old = frame(&[(2, 0, Bounds::new(1, 1, 15, 15))]);
    let new = frame(&[(2, 1, Bounds::new(20, 1, 33, 15))]);
    let mut damage = DamageTiles::new(new.physical_size);
    diff_frames(&old, &new, &mut damage, &mut RetainedDamage::default());
    assert_eq!(damage.list(), &[0, 1, 2]);
}

#[test]
fn removal_delta_is_resolved_before_the_same_id_is_reinserted() {
    let mut removed = frame(&[(2, 0, Bounds::new(16, 0, 32, 16))]);
    let old = removed.nodes[0];
    removed.delta = Some(std::rc::Rc::new(RetainedFrameDelta {
        from_version: 1,
        to_version: 2,
        patches: vec![RetainedNodePatch {
            old: Some(old),
            new: None,
            damage: None,
        }]
        .into(),
        previous: None,
        depth: 1,
        damage: std::rc::Rc::new([]),
        dirty_backdrops: std::rc::Rc::new([]),
        backdrop_damage_complete: false,
        index: std::rc::Rc::new([(old.id, 0)].into_iter().collect()),
    }));
    let reinserted = frame(&[(2, 0, Bounds::new(16, 0, 32, 16))]);
    let mut damage = DamageTiles::new(reinserted.physical_size);

    diff_frames(
        &removed,
        &reinserted,
        &mut damage,
        &mut RetainedDamage::default(),
    );

    assert_eq!(damage.list(), &[1]);
}

#[test]
fn overlay_only_insertion_participates_in_fallback_damage() {
    let previous = frame(&[]);
    let mut current = frame(&[]);
    let inserted = RetainedNodeState {
        id: RetainedNodeId::for_owner(2),
        revision: NodeGeneration::new(0),
        bounds: Bounds::new(32, 0, 48, 16),
        order: 0,
        kind: RetainedNodeKind::Scene,
        placement_bits: None,
    };
    current.delta = Some(std::rc::Rc::new(RetainedFrameDelta {
        from_version: 1,
        to_version: 2,
        patches: vec![RetainedNodePatch {
            old: None,
            new: Some(inserted),
            damage: None,
        }]
        .into(),
        previous: None,
        depth: 1,
        damage: std::rc::Rc::new([]),
        dirty_backdrops: std::rc::Rc::new([]),
        backdrop_damage_complete: false,
        index: std::rc::Rc::new([(inserted.id, 0)].into_iter().collect()),
    }));
    let mut damage = DamageTiles::new(current.physical_size);

    diff_frames(
        &previous,
        &current,
        &mut damage,
        &mut RetainedDamage::default(),
    );

    assert_eq!(damage.list(), &[2]);
}

#[test]
fn compacted_state_equal_to_the_rebuilt_base_adds_no_damage() {
    let mut previous = frame(&[(2, 0, Bounds::new(16, 0, 32, 16))]);
    let logical = RetainedNodeState {
        revision: NodeGeneration::new(1),
        ..previous.nodes[0]
    };
    previous.state_pages = std::rc::Rc::new(
        [(0, std::rc::Rc::<[RetainedNodeState]>::from(vec![logical]))]
            .into_iter()
            .collect(),
    );
    let current = frame(&[(2, 1, Bounds::new(16, 0, 32, 16))]);
    let mut damage = DamageTiles::new(current.physical_size);

    diff_frames(
        &previous,
        &current,
        &mut damage,
        &mut RetainedDamage::default(),
    );

    assert!(damage.is_empty());
}

#[test]
fn retained_layer_revision_change_dirties_its_bounds() {
    let mut previous = frame(&[(2, 0, Bounds::new(16, 0, 32, 16))]);
    Rc::make_mut(&mut previous.nodes)[0].kind = RetainedNodeKind::Layer;
    let mut current = previous.clone();
    Rc::make_mut(&mut current.nodes)[0].revision = NodeGeneration::new(1);
    let mut damage = DamageTiles::new(current.physical_size);

    diff_frames(
        &previous,
        &current,
        &mut damage,
        &mut RetainedDamage::default(),
    );

    assert_eq!(damage.list(), &[1]);
}

#[test]
fn insertion_does_not_make_unchanged_siblings_look_reordered() {
    let old = frame(&[
        (2, 0, Bounds::new(0, 0, 16, 16)),
        (3, 0, Bounds::new(32, 0, 48, 16)),
    ]);
    let new = frame(&[
        (4, 0, Bounds::new(16, 0, 32, 16)),
        (2, 0, Bounds::new(0, 0, 16, 16)),
        (3, 0, Bounds::new(32, 0, 48, 16)),
    ]);
    let mut damage = DamageTiles::new(new.physical_size);
    diff_frames(&old, &new, &mut damage, &mut RetainedDamage::default());
    assert_eq!(damage.list(), &[1]);
}

#[test]
fn removal_dirties_the_removed_nodes_old_tiles() {
    let old = frame(&[
        (2, 0, Bounds::new(0, 0, 16, 16)),
        (3, 0, Bounds::new(32, 16, 48, 32)),
    ]);
    let new = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
    let mut damage = DamageTiles::new(new.physical_size);
    diff_frames(&old, &new, &mut damage, &mut RetainedDamage::default());
    assert_eq!(damage.list(), &[10]);
}

#[test]
fn reordering_dirties_both_overlapping_nodes() {
    let old = frame(&[
        (2, 0, Bounds::new(0, 0, 32, 16)),
        (3, 0, Bounds::new(16, 0, 48, 16)),
    ]);
    let new = frame(&[
        (3, 0, Bounds::new(16, 0, 48, 16)),
        (2, 0, Bounds::new(0, 0, 32, 16)),
    ]);
    let mut damage = DamageTiles::new(new.physical_size);
    diff_frames(&old, &new, &mut damage, &mut RetainedDamage::default());
    assert_eq!(damage.list(), &[1]);
}

#[test]
fn reordered_pair_spanning_many_tiles_is_recorded_once() {
    let old = frame(&[
        (2, 0, Bounds::new(0, 0, 64, 64)),
        (3, 0, Bounds::new(16, 16, 80, 64)),
    ]);
    let new = frame(&[
        (3, 0, Bounds::new(16, 16, 80, 64)),
        (2, 0, Bounds::new(0, 0, 64, 64)),
    ]);
    let mut damage = DamageTiles::new(new.physical_size);
    let mut retained = RetainedDamage::default();

    diff_frames(&old, &new, &mut damage, &mut retained);

    assert_eq!(retained.unattributed, [Bounds::new(16, 16, 64, 64)]);
    assert_eq!(damage.len(), 9);
}

#[test]
fn reused_frame_diff_scratch_does_not_leak_old_tile_adjacency() {
    let overlapping = frame(&[
        (2, 0, Bounds::new(0, 0, 64, 64)),
        (3, 0, Bounds::new(16, 16, 80, 64)),
    ]);
    let overlapping_reversed = frame(&[
        (3, 0, Bounds::new(16, 16, 80, 64)),
        (2, 0, Bounds::new(0, 0, 64, 64)),
    ]);
    let mut scratch = FrameDiffScratch::default();
    diff_frames_reusing(
        &overlapping,
        &overlapping_reversed,
        &mut DamageTiles::new((128, 64)),
        &mut RetainedDamage::default(),
        &mut scratch,
    );

    let disjoint = frame(&[
        (2, 0, Bounds::new(0, 0, 16, 16)),
        (3, 0, Bounds::new(32, 0, 48, 16)),
    ]);
    let disjoint_reversed = frame(&[
        (3, 0, Bounds::new(32, 0, 48, 16)),
        (2, 0, Bounds::new(0, 0, 16, 16)),
    ]);
    let mut damage = DamageTiles::new((128, 64));
    let mut retained = RetainedDamage::default();
    diff_frames_reusing(
        &disjoint,
        &disjoint_reversed,
        &mut damage,
        &mut retained,
        &mut scratch,
    );

    assert!(damage.is_empty());
    assert!(retained.unattributed.is_empty());
}

#[test]
fn reordering_non_overlapping_nodes_does_not_create_damage() {
    let old = frame(&[
        (2, 0, Bounds::new(0, 0, 16, 16)),
        (3, 0, Bounds::new(32, 0, 48, 16)),
    ]);
    let new = frame(&[
        (3, 0, Bounds::new(32, 0, 48, 16)),
        (2, 0, Bounds::new(0, 0, 16, 16)),
    ]);
    let mut damage = DamageTiles::new(new.physical_size);
    diff_frames(&old, &new, &mut damage, &mut RetainedDamage::default());
    assert!(damage.is_empty());
}

#[test]
fn spatial_reorder_matches_brute_force_for_every_ordering() {
    let nodes = [
        (2, 0, Bounds::new(-8, 0, 12, 24)),
        (3, 0, Bounds::new(4, 8, 28, 32)),
        (4, 0, Bounds::new(24, 0, 44, 20)),
        (5, 0, Bounds::new(36, 12, 60, 36)),
        (6, 0, Bounds::new(8, 28, 52, 52)),
    ];
    let previous = frame(&nodes);
    let old_rank = previous
        .nodes
        .iter()
        .enumerate()
        .map(|(rank, node)| (node.id, rank))
        .collect::<HashMap<_, _>>();
    let bounds = previous
        .nodes
        .iter()
        .map(|node| (node.id, node.bounds))
        .collect::<HashMap<_, _>>();
    let mut ids = nodes.map(|node| node.0);

    for_each_permutation(&mut ids, 0, &mut |order| {
        let current = frame(
            &order
                .iter()
                .map(|id| (*id, 0, bounds[&RetainedNodeId::for_owner(*id)]))
                .collect::<Vec<_>>(),
        );
        let mut actual = DamageTiles::new(current.physical_size);
        diff_frames(
            &previous,
            &current,
            &mut actual,
            &mut RetainedDamage::default(),
        );

        let mut expected = DamageTiles::new(current.physical_size);
        for (new_rank_a, a) in current.nodes.iter().enumerate() {
            for b in &current.nodes[new_rank_a + 1..] {
                if old_rank[&a.id] > old_rank[&b.id] {
                    expected.add_bounds(a.bounds.intersect(b.bounds));
                }
            }
        }
        let mut actual = actual.list().to_vec();
        let mut expected = expected.list().to_vec();
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected, "order {order:?}");
    });
}

fn for_each_permutation(values: &mut [u64], index: usize, visit: &mut dyn FnMut(&[u64])) {
    if index == values.len() {
        visit(values);
        return;
    }
    for next in index..values.len() {
        values.swap(index, next);
        for_each_permutation(values, index + 1, visit);
        values.swap(index, next);
    }
}
