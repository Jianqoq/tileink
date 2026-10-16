use super::*;

#[test]
fn content_revision_reuses_chunk_canvas_storage() {
    let root = RetainedNodeId::for_owner(5);
    let child = RetainedNodeId::for_owner(6);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            Affine::translate((8.0, 8.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let chunk_storage = std::ptr::from_ref(&materializer.chunks[&child]);
    let storage = std::ptr::from_ref(&materializer.chunks[&child].canvas);
    let plan = materializer.chunks[&child]
        .canvas
        .execution_plan_fingerprint();

    scene
        .transaction()
        .replace_scene(child, leaf(Color::BLACK))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_eq!(
        std::ptr::from_ref(&materializer.chunks[&child]),
        chunk_storage
    );
    assert_eq!(
        std::ptr::from_ref(&materializer.chunks[&child].canvas),
        storage
    );
    assert_eq!(
        materializer.chunks[&child]
            .canvas
            .execution_plan_fingerprint(),
        plan
    );
    assert!(materializer.chunks[&child].backdrop_dependencies.is_empty());
    assert!(!materializer.nonlocal_dependencies.contains(&child));
    assert!(!materializer.surface_dependent_plans.contains(&child));
    assert_eq!(materializer.chunks[&child].generation, 1);
}

#[test]
fn materialized_dirty_range_buffers_rotate_between_frames() {
    let root = RetainedNodeId::for_owner(62_077);
    let child = RetainedNodeId::for_owner(62_078);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let initial = materializer.canvas.buffer_changes.as_ref().unwrap();
    assert!(!initial.draws.is_empty());
    let initial_draw_ranges = initial.draws.as_ptr();

    scene
        .transaction()
        .replace_scene(child, leaf(Color::BLACK))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(
        !materializer
            .canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .draws
            .is_empty()
    );

    scene
        .transaction()
        .replace_scene(child, leaf(Color::WHITE))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let third = materializer.canvas.buffer_changes.as_ref().unwrap();

    // SceneArena and SceneBufferChanges exchange their allocations each sync. Equal-sized edits
    // therefore return to the first frame's range buffer after one intervening frame.
    assert_eq!(third.draws.as_ptr(), initial_draw_ranges);
}

#[test]
fn cached_node_draw_order_tracks_content_and_arena_relocation() {
    let root = RetainedNodeId::for_owner(62_080);
    let removed_a = RetainedNodeId::for_owner(62_081);
    let removed_b = RetainedNodeId::for_owner(62_082);
    let retained = RetainedNodeId::for_owner(62_083);
    let inserted = RetainedNodeId::for_owner(62_084);
    let mut scene = RetainedScene::new(256, 256, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            removed_a,
            multi_draw_leaf(4),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            removed_b,
            multi_draw_leaf(4),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            retained,
            multi_draw_leaf(4),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let before = materializer
        .node_physical_draws(retained)
        .collect::<Vec<_>>();
    let compactions = materializer.arenas.draws.compactions();

    scene
        .transaction()
        .remove_subtree(removed_a)
        .remove_subtree(removed_b)
        .insert_scene(
            RetainedParent::content(root),
            None,
            inserted,
            multi_draw_leaf(20),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(materializer.arenas.draws.compactions() > compactions);
    let after = materializer
        .node_physical_draws(retained)
        .collect::<Vec<_>>();
    assert_ne!(after, before);
    let chunk = &materializer.chunks[&retained];
    let base = materializer.arenas.draws.range(chunk.draws).start;
    let expected = chunk
        .canvas
        .compile(crate::shared::execution::ROOT_COMMAND_LIST_ID)
        .draw_order
        .iter()
        .map(|&draw| base + draw as usize)
        .collect::<Vec<_>>();
    assert_eq!(after, expected);

    scene
        .transaction()
        .replace_scene(retained, multi_draw_leaf(6))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let after_replacement = materializer
        .node_physical_draws(retained)
        .collect::<Vec<_>>();
    assert_eq!(after_replacement.len(), 6);
    let chunk = &materializer.chunks[&retained];
    let base = materializer.arenas.draws.range(chunk.draws).start;
    let expected = chunk
        .canvas
        .compile(crate::shared::execution::ROOT_COMMAND_LIST_ID)
        .draw_order
        .iter()
        .map(|&draw| base + draw as usize)
        .collect::<Vec<_>>();
    assert_eq!(after_replacement, expected);
}

#[test]
fn empty_chunk_allocations_grow_and_shrink_without_full_sync() {
    let root = RetainedNodeId::for_owner(7);
    let child = RetainedNodeId::for_owner(8);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            empty_leaf(),
            Affine::translate((4.0, 4.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let chunk = &materializer.chunks[&child];
    assert!(materializer.arenas.draws.range(chunk.draws).is_empty());
    assert!(materializer.arenas.sdfs.range(chunk.sdfs).is_empty());

    scene
        .transaction()
        .replace_scene(child, leaf(Color::WHITE))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let chunk = &materializer.chunks[&child];
    assert_eq!(materializer.arenas.draws.range(chunk.draws).len(), 1);
    assert!(!materializer.arenas.sdfs.range(chunk.sdfs).is_empty());

    scene
        .transaction()
        .replace_scene(child, empty_leaf())
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let chunk = &materializer.chunks[&child];
    assert!(materializer.arenas.draws.range(chunk.draws).is_empty());
    assert!(materializer.arenas.sdfs.range(chunk.sdfs).is_empty());
}
