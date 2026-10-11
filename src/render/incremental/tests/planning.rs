use super::*;

#[test]
fn journal_removal_propagates_old_pixels_through_backdrop() {
    let old_bounds = Bounds::new(32, 16, 48, 32);
    let mut previous = frame(&[(2, 0, old_bounds)]);
    previous.version = 1;
    let old = previous.nodes[0];
    let mut current = frame(&[]);
    current.version = 2;
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
fn indexed_delta_omits_unused_oracle_sources_without_losing_damage() {
    // Root tile coverage and indexed backdrop invalidation are independent of
    // the command-tree oracle. Bulk removals must not build unused source lists.
    for requires_propagation in [false, true] {
        for indexed_backdrops in [false, true] {
            let mut previous = frame(&[(2, 0, Bounds::new(0, 0, 16, 16))]);
            previous.version = 1;
            let old = previous.nodes[0];
            let mut current = frame(&[(3, 0, Bounds::new(16, 0, 32, 16))]);
            current.version = 2;
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
