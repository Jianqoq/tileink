use super::*;
use crate::{
    NodeGeneration, RetainedNodeId,
    canvas::{RetainedFrameDelta, RetainedNodeKind, RetainedNodePatch, RetainedNodeState},
};

fn frame(nodes: &[(u64, u64, Bounds)]) -> RetainedFrame {
    let nodes = nodes
        .iter()
        .enumerate()
        .map(|(order, (id, revision, bounds))| RetainedNodeState {
            id: RetainedNodeId::for_owner(*id),
            revision: NodeGeneration::new(*revision),
            bounds: *bounds,
            order: order as u32,
            kind: RetainedNodeKind::Scene,
            placement_bits: None,
        })
        .collect::<Vec<_>>();
    let node_index = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id, index))
        .collect();
    RetainedFrame {
        root: RetainedNodeId::for_owner(1),
        logical_size: (128, 64),
        physical_size: (128, 64),
        scale_bits: 1.0f32.to_bits(),
        nodes: nodes.into(),
        node_index: std::rc::Rc::new(node_index),
        state_pages: std::rc::Rc::new(Default::default()),
        invalidated_bounds: Vec::new(),
        invalidate_all: false,
        incremental_complete: true,
        version: None,
        delta: None,
        damage_history: crate::canvas::damage_history::DamageHistory::default(),
        dependency_free: false,
        requires_damage_propagation: true,
    }
}

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
fn incomplete_previous_frame_cannot_become_an_incremental_baseline() {
    let mut previous = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
    previous.incremental_complete = false;
    let current = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
    let mut state = IncrementalState {
        previous: Some(previous),
        renderer_state_invalid: false,
        frame_diff: Default::default(),
    };

    let plan = state.plan(
        Some(current),
        (128, 64),
        IncrementalRenderConfig::default(),
        true,
    );

    assert_eq!(
        plan.stats.full_redraw_reason,
        Some(FullRedrawReason::UntrackedContent)
    );
    assert_eq!(plan.stats.dirty_tiles, plan.stats.total_tiles);
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
fn journal_removal_propagates_old_pixels_through_backdrop() {
    let old_bounds = Bounds::new(32, 16, 48, 32);
    let mut previous = frame(&[(2, 0, old_bounds)]);
    previous.version = Some(1);
    let old = previous.nodes[0];
    let mut current = frame(&[]);
    current.version = Some(2);
    current.delta = Some(Rc::new(RetainedFrameDelta {
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
        damage: Rc::new([]),
        dirty_backdrops: Rc::new([]),
        backdrop_damage_complete: false,
        index: Rc::new([(old.id, 0)].into_iter().collect()),
    }));
    let mut state = IncrementalState::default();
    state.commit(Some(previous));
    let plan = state.plan(
        Some(current),
        (128, 64),
        IncrementalRenderConfig::default(),
        true,
    );
    assert!(!plan.stats.full_redraw);
    assert_eq!(plan.changed_tiles.list(), &[10]);
    for domain in 0..4 {
        let mut scene = crate::Canvas::new(128, 64, 1.0);
        let rect = peniko::kurbo::Rect::new(0.0, 0.0, 128.0, 64.0);
        let region = crate::Region::rect(rect, crate::Radius::ZERO);
        match domain {
            1 => scene.push_filter_layer(crate::Filter::Opacity(0.75), region),
            2 => scene.push_isolate_layer(
                peniko::kurbo::Shape::to_path(&rect, 0.1),
                peniko::kurbo::Affine::IDENTITY,
                0.1,
            ),
            3 => {
                let mut mask = crate::Canvas::new(128, 64, 1.0);
                mask.push_rect(rect, crate::Radius::ZERO, peniko::Color::WHITE);
                scene.push_mask_layer(
                    mask,
                    crate::Mask {
                        region,
                        kind: crate::MaskKind::Alpha,
                    },
                );
            }
            _ => {}
        }
        scene.push_backdrop_layer(
            crate::Filter::Blur {
                std_dev_x: 2.0,
                std_dev_y: 2.0,
                sampling: Default::default(),
            },
            crate::Region::rect(
                peniko::kurbo::Rect::new(0.0, 0.0, 128.0, 64.0),
                crate::Radius::ZERO,
            ),
        );
        scene.pop_layer();
        if domain != 0 {
            scene.pop_layer();
        }
        // The deleted ID has no command in the new scene. Its old pixels still
        // affect the following blur, including pixels outside the removed tile.
        let propagated = scene.propagate_damage(&plan.retained_damage);
        assert!(
            propagated.bounds.contains(&Bounds::new(26, 10, 54, 38)),
            "domain={domain}: {:?}",
            propagated.bounds
        );
    }
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

#[test]
fn dirty_threshold_switches_to_the_full_fast_path() {
    let old = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
    let new = frame(&[(2, 1, Bounds::new(0, 0, 96, 64))]);
    let mut state = IncrementalState {
        previous: Some(old),
        renderer_state_invalid: false,
        frame_diff: Default::default(),
    };
    let plan = state.plan(
        Some(new),
        (128, 64),
        IncrementalRenderConfig {
            full_redraw_ratio: 0.7,
            ..Default::default()
        },
        true,
    );
    assert_eq!(
        plan.stats.full_redraw_reason,
        Some(FullRedrawReason::DirtyTileThreshold)
    );
    assert_eq!(plan.stats.dirty_tiles, plan.stats.total_tiles);
}

#[test]
fn stale_history_preserves_scene_damage_for_output_strategy() {
    let old = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
    let new = frame(&[(2, 1, Bounds::new(16, 0, 32, 16))]);
    let mut state = IncrementalState {
        previous: Some(old),
        renderer_state_invalid: false,
        frame_diff: Default::default(),
    };

    let plan = state.plan(
        Some(new),
        (128, 64),
        IncrementalRenderConfig::default(),
        false,
    );

    assert_eq!(
        plan.stats.full_redraw_reason,
        Some(FullRedrawReason::FirstFrame)
    );
    assert_eq!(plan.stats.dirty_tiles, plan.stats.total_tiles);
    assert_eq!(plan.stats.changed_tiles, 2);
}

#[test]
fn transient_output_uses_hysteresis_before_rebuilding_history() {
    let config = IncrementalRenderConfig::default();
    let stats = |changed_ratio, reason| IncrementalRenderStats {
        changed_ratio,
        full_redraw_reason: reason,
        ..Default::default()
    };
    let mut state = TransientOutputState::default();

    assert_eq!(
        state.decide(&stats(1.0, Some(FullRedrawReason::SurfaceChanged)), config),
        TransientOutputDecision::Direct
    );
    assert_eq!(
        state.decide(&stats(0.1, Some(FullRedrawReason::FirstFrame)), config),
        TransientOutputDecision::Direct
    );
    assert_eq!(
        state.decide(&stats(0.5, Some(FullRedrawReason::FirstFrame)), config),
        TransientOutputDecision::Direct
    );
    assert_eq!(
        state.decide(&stats(0.0, Some(FullRedrawReason::FirstFrame)), config),
        TransientOutputDecision::Direct
    );
    assert_eq!(
        state.decide(&stats(0.0, Some(FullRedrawReason::FirstFrame)), config),
        TransientOutputDecision::RebuildHistory
    );
}

#[test]
fn first_frame_builds_history_instead_of_starting_direct_mode() {
    let mut state = TransientOutputState::default();
    let stats = IncrementalRenderStats {
        changed_ratio: 1.0,
        full_redraw_reason: Some(FullRedrawReason::FirstFrame),
        ..Default::default()
    };
    assert_eq!(
        state.decide(&stats, IncrementalRenderConfig::default()),
        TransientOutputDecision::InternalHistory
    );
}

#[test]
fn surface_change_forces_full_redraw() {
    let old = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
    let mut resized = old.clone();
    resized.logical_size = (144, 64);
    resized.physical_size = (144, 64);
    let mut state = IncrementalState {
        previous: Some(old),
        renderer_state_invalid: false,
        frame_diff: Default::default(),
    };
    let plan = state.plan(
        Some(resized),
        (144, 64),
        IncrementalRenderConfig::default(),
        true,
    );
    assert_eq!(
        plan.stats.full_redraw_reason,
        Some(FullRedrawReason::SurfaceChanged)
    );
}

#[test]
fn renderer_invalidation_reports_its_own_fallback_reason() {
    let retained = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
    let mut state = IncrementalState {
        previous: Some(retained.clone()),
        renderer_state_invalid: true,
        frame_diff: Default::default(),
    };
    let plan = state.plan(
        Some(retained),
        (128, 64),
        IncrementalRenderConfig::default(),
        false,
    );
    assert_eq!(
        plan.stats.full_redraw_reason,
        Some(FullRedrawReason::RendererStateChanged)
    );
}

#[test]
fn active_scan_uses_persistent_chunk_allocations_after_a_path_is_removed() {
    use peniko::{
        Color,
        kurbo::{Affine, Rect, Shape},
    };

    use crate::{Canvas, FillRule, shared::gpu_plan::PersistentPathPlans};

    let mut canvas = Canvas::new(64, 16, 1.0);
    for x in [0.0, 16.0, 32.0] {
        canvas.push_path(
            Rect::new(x, 0.0, x + 12.0, 16.0).to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
    }
    let mut plans = PersistentPathPlans::default();
    plans.update(&canvas, None);
    canvas.path_records[1] = Default::default();
    plans.update(&canvas, Some(std::slice::from_ref(&(1..2))));

    let mut damage = DamageTiles::new((64, 16));
    damage.add_bounds(Bounds::new(32, 0, 48, 16));
    let active = ActiveScanPlan::new(&canvas, &damage, plans.scan_ranges());
    let record = &canvas.path_records[2];
    let part = |base: u32, count: u32| &active.indices[base as usize..(base + count) as usize];
    assert_eq!(
        part(active.line_base, active.line_count),
        (record.line_start..record.line_start + record.line_count).collect::<Vec<_>>()
    );
    assert_eq!(part(active.path_base, active.path_count), &[record.path_id]);
    assert_eq!(
        part(active.backdrop_base, active.backdrop_count),
        (record.data_offset..record.data_offset + record.data_len).collect::<Vec<_>>()
    );
    assert_eq!(
        active.cumsum.chunk_lens.iter().sum::<u32>(),
        record.data_len
    );
    assert_eq!(
        active.cumsum.chunk_backdrop_offsets.first(),
        Some(&record.data_offset)
    );
    let chunks = &active.indices
        [active.chunk_base as usize..(active.chunk_base + active.chunk_count) as usize];

    assert_eq!(
        chunks,
        (plans.scan_ranges()[2].start..plans.scan_ranges()[2].end).collect::<Vec<_>>()
    );
}

#[test]
fn indexed_delta_omits_unused_oracle_sources_without_losing_damage() {
    // Root tile coverage and indexed backdrop invalidation are independent of
    // the command-tree oracle. Bulk removals must not build unused source lists.
    for requires_propagation in [false, true] {
        for indexed_backdrops in [false, true] {
            let mut previous = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
            previous.version = Some(1);
            let old = previous.nodes[0];
            let mut current = frame(&[(3, 0, Bounds::new(16, 0, 32, 16))]);
            current.version = Some(2);
            current.requires_damage_propagation = requires_propagation;
            current.invalidated_bounds.push(Bounds::new(48, 0, 64, 16));
            let new = current.nodes[0];
            let backdrop = RetainedNodeId::for_owner(9);
            current.delta = Some(Rc::new(RetainedFrameDelta {
                from_version: 1,
                to_version: 2,
                patches: vec![
                    RetainedNodePatch {
                        old: Some(old),
                        new: None,
                        damage: None,
                    },
                    RetainedNodePatch {
                        old: None,
                        new: Some(new),
                        damage: None,
                    },
                ]
                .into(),
                previous: None,
                depth: 1,
                damage: vec![(new.id, Bounds::new(32, 0, 48, 16))].into(),
                dirty_backdrops: vec![backdrop].into(),
                backdrop_damage_complete: indexed_backdrops,
                index: Rc::new([(old.id, 0), (new.id, 1)].into_iter().collect()),
            }));
            let mut state = IncrementalState::default();
            state.commit(Some(previous));
            let plan = state.plan(
                Some(current),
                (128, 64),
                IncrementalRenderConfig::default(),
                true,
            );
            assert!(!plan.stats.full_redraw);
            assert_eq!(plan.changed_tiles.list(), &[0, 1, 2, 3]);
            assert_eq!(
                plan.dirty_backdrops.as_deref(),
                indexed_backdrops.then_some(&[backdrop][..])
            );
            if requires_propagation && !indexed_backdrops {
                assert_eq!(
                    plan.retained_damage.unattributed,
                    vec![Bounds::new(0, 0, 16, 16), Bounds::new(48, 0, 64, 16)]
                );
                assert_eq!(
                    plan.retained_damage.node_bounds.get(&new.id),
                    Some(&Bounds::new(16, 0, 48, 16))
                );
            } else {
                assert!(plan.retained_damage.unattributed.is_empty());
                assert!(plan.retained_damage.node_bounds.is_empty());
            }
        }
    }
}
