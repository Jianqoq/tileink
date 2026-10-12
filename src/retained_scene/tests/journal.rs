use super::*;

#[test]
fn journal_merges_skipped_versions() {
    let root = RetainedNodeId::for_owner(1);
    let child = RetainedNodeId::for_owner(2);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    transaction.insert_scene(
        RetainedParent::content(root),
        None,
        child,
        leaf(Color::WHITE),
        Affine::translate((0.0, 0.0)),
    );
    transaction.commit().unwrap();
    let mut transaction = scene.transaction();
    transaction.set_transform(child, Affine::translate((4.0, 5.0)));
    transaction.commit().unwrap();
    let changes = scene.changes_since(SceneVersion::INITIAL).unwrap();
    assert!(changes.changed_nodes.contains(&child));
    assert!(changes.topology_changed);
}

#[test]
fn journal_gap_requires_one_full_resynchronization() {
    let root = RetainedNodeId::for_owner(1);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    for _ in 0..=JOURNAL_CAPACITY {
        scene
            .transaction()
            .invalidate_rect(Rect::new(0.0, 0.0, 1.0, 1.0))
            .commit()
            .unwrap();
    }

    assert!(scene.changes_since(SceneVersion::INITIAL).is_none());
    let recent = SceneVersion(scene.version().get() - 1);
    assert!(scene.changes_since(recent).is_some());
}

#[test]
fn journal_gap_detects_remove_and_reinsert_of_the_same_node_id() {
    let root = RetainedNodeId::for_owner(81_000);
    let child = RetainedNodeId::for_owner(81_001);
    let first = leaf(Color::WHITE);
    let replacement = leaf(Color::BLACK);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            first,
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);

    scene.transaction().remove_subtree(child).commit().unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            replacement.clone(),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    for _ in 0..255 {
        scene.transaction().invalidate_all().commit().unwrap();
    }

    assert!(update_materializer(&mut materializer, &scene));
    assert!(Rc::ptr_eq(
        &materializer.chunks[&child]
            .placement
            .as_ref()
            .unwrap()
            .source,
        &replacement
    ));
    assert!(
        materializer
            .canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .full_scene_sync
    );
}

#[test]
fn long_content_delta_chain_compacts_into_state_pages() {
    let root = RetainedNodeId::for_owner(82_000);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let content = leaf(Color::WHITE);
    let mut transaction = scene.transaction();
    for index in 0..256 {
        transaction.insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(82_001 + index),
            content.clone(),
            Affine::translate((0.0, 0.0)),
        );
    }
    transaction.commit().unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let base_nodes = materializer
        .canvas
        .persistent_frame
        .as_ref()
        .unwrap()
        .nodes
        .clone();

    for index in 0..256 {
        scene
            .transaction()
            .replace_scene(RetainedNodeId::for_owner(82_001 + index), content.clone())
            .commit()
            .unwrap();
        assert!(update_materializer(&mut materializer, &scene));
    }

    let frame = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(Rc::ptr_eq(&frame.nodes, &base_nodes));
    assert!(!frame.state_pages.is_empty());
    assert_eq!(frame.delta.as_ref().unwrap().depth, 1);
    assert!(frame.delta.as_ref().unwrap().previous.is_none());
    assert_eq!(
        frame.node_revision(RetainedNodeId::for_owner(82_001)),
        Some(NodeRevision::new(
            scene.nodes[&RetainedNodeId::for_owner(82_001)].instance,
            1
        ))
    );
}
