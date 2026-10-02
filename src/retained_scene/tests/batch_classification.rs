use super::*;

fn content(backdrop: bool, color: Color) -> Rc<Canvas> {
    if backdrop {
        backdrop_leaf()
    } else {
        leaf(color)
    }
}

fn scene_with_pair(first: Rc<Canvas>, second: Rc<Canvas>) -> (RetainedScene, [RetainedNodeId; 2]) {
    let root = RetainedNodeId::for_owner(71_000);
    let ids = [
        RetainedNodeId::for_owner(71_001),
        RetainedNodeId::for_owner(71_002),
    ];
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            ids[0],
            first,
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            ids[1],
            second,
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    (scene, ids)
}

fn assert_painter_metadata_matches_fresh(
    scene: &RetainedScene,
    materializer: &PersistentSceneMaterializer,
    ids: &[RetainedNodeId],
) {
    let fresh = PersistentSceneMaterializer::new(scene);
    assert_eq!(
        root_retained_commands(materializer),
        root_retained_commands(&fresh)
    );
    // Physical slots and numeric batch IDs depend on allocation history. Compare logical
    // painter order and batch equivalence classes, preserving inactive draws explicitly.
    let metadata = |state: &PersistentSceneMaterializer| {
        let mut known_batches = Vec::new();
        let mut result = Vec::new();
        for &id in ids {
            let draws = state
                .node_physical_draws(id)
                .map(|physical| {
                    let key = state.canvas.painter_keys.as_ref().unwrap()[physical].clone();
                    let batch = state.canvas.stable_batch_ids.as_ref().unwrap()[physical];
                    let plan = state.canvas.compiled_plan.as_ref().unwrap();
                    assert_eq!(
                        batch,
                        plan.draw_batch_ids
                            .get(physical)
                            .copied()
                            .unwrap_or(u32::MAX)
                    );
                    let group = (batch != u32::MAX).then(|| {
                        known_batches
                            .iter()
                            .position(|&known| known == batch)
                            .unwrap_or_else(|| {
                                known_batches.push(batch);
                                known_batches.len() - 1
                            })
                    });
                    (key, group)
                })
                .collect::<Vec<_>>();
            let bounds = state
                .canvas
                .persistent_frame
                .as_ref()
                .unwrap()
                .node_state(id)
                .map(|node| node.bounds);
            result.push((draws, bounds));
        }
        result
    };
    assert_eq!(metadata(materializer), metadata(&fresh));
}

#[test]
fn mixed_content_revisions_match_fresh_painter_metadata() {
    // Folding classification into chunk rebuilding must still classify the OLD chunk.
    // Real content transitions exercise both sides without exposing the private predicate.
    for before in [false, true] {
        for after in [false, true] {
            let (mut scene, ids) =
                scene_with_pair(content(before, Color::WHITE), leaf(Color::WHITE));
            let mut materializer = PersistentSceneMaterializer::new(&scene);
            scene
                .transaction()
                .replace_scene(ids[0], content(after, Color::BLACK))
                .replace_scene(ids[1], leaf(Color::BLACK))
                .commit()
                .unwrap();
            let changes = scene.changes_since(materializer.version()).unwrap();
            assert!(!changes.topology_changed);
            assert!(ids.iter().all(|id| changes.changed_nodes.contains(id)));
            assert!(materializer.update(&scene, Some(changes)));
            assert_painter_metadata_matches_fresh(&scene, &materializer, &ids);
        }
    }
}

#[test]
fn mixed_change_set_preserves_an_unchanged_generation() {
    for backdrop in [false, true] {
        let (mut scene, ids) = scene_with_pair(leaf(Color::WHITE), content(backdrop, Color::WHITE));
        let mut materializer = PersistentSceneMaterializer::new(&scene);
        let unchanged = (
            scene.nodes[&ids[1]].instance,
            scene.nodes[&ids[1]].generation,
        );
        scene
            .transaction()
            .replace_scene(ids[0], leaf(Color::BLACK))
            .commit()
            .unwrap();
        let mut changes = scene.changes_since(materializer.version()).unwrap();
        assert!(!changes.topology_changed);
        assert!(!changes.changed_nodes.contains(&ids[1]));
        // A conservative change snapshot may include an unchanged node. A no-op setter
        // cannot produce this case: it emits no journal entry. Preserve the real chunk
        // and exercise classification before the generation-equality early exit.
        changes.changed_nodes.insert(ids[1]);
        assert_eq!(
            (
                materializer.chunks[&ids[1]].instance,
                materializer.chunks[&ids[1]].generation
            ),
            unchanged
        );
        assert!(materializer.update(&scene, Some(changes)));
        assert_eq!(
            (
                materializer.chunks[&ids[1]].instance,
                materializer.chunks[&ids[1]].generation
            ),
            unchanged
        );
        assert_painter_metadata_matches_fresh(&scene, &materializer, &ids);
    }
}

#[test]
fn content_classification_changes_preserve_unchanged_members() {
    // Reusing OLD classification is valid only when no plan change remains unhandled.
    // Exercise both transition directions with a conservative unchanged journal member.
    for before in [false, true] {
        for after in [false, true] {
            for unchanged_backdrop in [false, true] {
                let (mut scene, ids) = scene_with_pair(
                    content(before, Color::WHITE),
                    content(unchanged_backdrop, Color::WHITE),
                );
                let mut materializer = PersistentSceneMaterializer::new(&scene);
                scene
                    .transaction()
                    .replace_scene(ids[0], content(after, Color::BLACK))
                    .commit()
                    .unwrap();
                let mut changes = scene.changes_since(materializer.version()).unwrap();
                assert!(!changes.topology_changed);
                changes.changed_nodes.insert(ids[1]);
                assert!(materializer.update(&scene, Some(changes)));
                assert_painter_metadata_matches_fresh(&scene, &materializer, &ids);
            }
        }
    }
}

#[test]
fn resize_and_content_classification_restore_exact_metadata() {
    for before in [false, true] {
        for after in [false, true] {
            let mut path = Canvas::new(96, 32, 1.0);
            path.push_path(
                Rect::new(8.0, 4.0, 88.0, 28.0).to_path(0.1),
                Color::WHITE,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
            );
            let (mut scene, ids) = scene_with_pair(Rc::new(path), content(before, Color::WHITE));
            scene.transaction().resize(32, 32, 1.0).commit().unwrap();
            let mut materializer = PersistentSceneMaterializer::new(&scene);
            assert_eq!(
                materializer.chunks[&ids[0]].canvas.path_records[0].tile_x1,
                2
            );
            scene
                .transaction()
                .resize(96, 32, 1.0)
                .replace_scene(ids[1], content(after, Color::BLACK))
                .commit()
                .unwrap();
            assert!(update_materializer(&mut materializer, &scene));
            assert_eq!(
                materializer.chunks[&ids[0]].canvas.path_records[0].tile_x1,
                6
            );
            // Resize deliberately defers exact damage metadata until a non-resize update.
            scene
                .transaction()
                .replace_scene(ids[1], content(after, Color::WHITE))
                .set_transform(ids[0], Affine::translate((1.0, 0.0)))
                .commit()
                .unwrap();
            assert!(update_materializer(&mut materializer, &scene));
            assert_painter_metadata_matches_fresh(&scene, &materializer, &ids);
        }
    }
}

#[test]
fn content_replacement_compaction_keeps_batch_membership_and_order() {
    let (mut scene, pair) = scene_with_pair(multi_draw_leaf(4), multi_draw_leaf(4));
    let third = RetainedNodeId::for_owner(71_003);
    let parent = RetainedParent::content(scene.root);
    scene
        .transaction()
        .insert_scene(parent, None, third, multi_draw_leaf(4), Affine::IDENTITY)
        .commit()
        .unwrap();
    let ids = [pair[0], pair[1], third];
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    scene
        .transaction()
        .replace_scene(pair[0], multi_draw_leaf(0))
        .replace_scene(pair[1], multi_draw_leaf(0))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let compactions = materializer.arenas.draws.compactions();
    scene
        .transaction()
        .replace_scene(third, multi_draw_leaf(20))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(materializer.arenas.draws.compactions() > compactions);
    assert_painter_metadata_matches_fresh(&scene, &materializer, &ids);
    // The following same-plan edit must reuse the newly compacted metadata correctly.
    scene
        .transaction()
        .replace_scene(third, multi_draw_leaf(20))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_painter_metadata_matches_fresh(&scene, &materializer, &ids);
}

#[test]
fn position_patch_and_mixed_plan_replacement_match_fresh_batches() {
    let (mut scene, ids) = scene_with_pair(backdrop_leaf(), leaf(Color::WHITE));
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    scene
        .transaction()
        .set_transform(ids[0], Affine::translate((4.0, 3.0)))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(
        materializer
            .canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .plan_values_patched
    );
    assert_painter_metadata_matches_fresh(&scene, &materializer, &ids);
    // An unpatchable companion prevents treating the whole update as a position patch.
    scene
        .transaction()
        .set_transform(ids[0], Affine::translate((8.0, 5.0)))
        .replace_scene(ids[1], backdrop_leaf())
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(
        !materializer
            .canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .plan_values_patched
    );
    assert_painter_metadata_matches_fresh(&scene, &materializer, &ids);
}
