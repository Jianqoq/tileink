use super::*;

#[test]
fn transaction_is_atomic_when_a_late_mutation_is_invalid() {
    let root = RetainedNodeId::for_owner(1);
    let child = RetainedNodeId::for_owner(2);
    let missing = RetainedNodeId::for_owner(9);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    transaction
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .reparent(child, RetainedParent::content(missing), None);
    assert_eq!(
        transaction.commit(),
        Err(RetainedSceneError::MissingNode(missing))
    );
    assert_eq!(scene.version(), SceneVersion::INITIAL);
    assert!(!scene.nodes.contains_key(&child));
}

#[test]
fn failed_transaction_restores_order_keys_after_rebalance() {
    let root = RetainedNodeId::for_owner(10);
    let first = RetainedNodeId::for_owner(11);
    let tail = RetainedNodeId::for_owner(12);
    let missing = RetainedNodeId::for_owner(u64::MAX);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            first,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            tail,
            leaf(Color::BLACK),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    let mut owner = 13;
    loop {
        let children = &scene.nodes[&root].content;
        let upper = children.key_of(tail).unwrap();
        let lower = children.order.range(..upper).next_back().unwrap().0;
        if upper - lower == 1 {
            break;
        }
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                Some(tail),
                RetainedNodeId::for_owner(owner),
                leaf(Color::WHITE),
                Affine::translate((0.0, 0.0)),
            )
            .commit()
            .unwrap();
        owner += 1;
    }

    let order_before = scene.nodes[&root].content.order.clone();
    let keys_before = scene.nodes[&root].content.keys.clone();
    let inserted = RetainedNodeId::for_owner(owner);
    let mut transaction = scene.transaction();
    transaction
        .insert_scene(
            RetainedParent::content(root),
            Some(tail),
            inserted,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .reparent(inserted, RetainedParent::content(missing), None);

    assert_eq!(
        transaction.commit(),
        Err(RetainedSceneError::MissingNode(missing))
    );
    assert_eq!(scene.nodes[&root].content.order, order_before);
    assert_eq!(scene.nodes[&root].content.keys, keys_before);

    let mut transaction = scene.transaction();
    transaction
        .move_before(first, tail)
        .reparent(tail, RetainedParent::content(missing), None);
    assert_eq!(
        transaction.commit(),
        Err(RetainedSceneError::MissingNode(missing))
    );
    assert_eq!(scene.nodes[&root].content.order, order_before);
    assert_eq!(scene.nodes[&root].content.keys, keys_before);
}

#[test]
fn undo_log_restores_values_order_removals_and_surface_after_late_failure() {
    let root = RetainedNodeId::for_owner(10);
    let a = RetainedNodeId::for_owner(11);
    let b = RetainedNodeId::for_owner(12);
    let invalid = RetainedNodeId::for_owner(13);
    let original = leaf(Color::WHITE);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            a,
            std::rc::Rc::clone(&original),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            b,
            leaf(Color::BLACK),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    let version = scene.version();
    let order = scene.nodes[&root]
        .content
        .order
        .iter()
        .map(|(&key, &id)| (key, id))
        .collect::<Vec<_>>();

    let result = scene
        .transaction()
        .replace_scene(a, leaf(Color::from_rgb8(20, 80, 220)))
        .move_before(b, a)
        .remove_subtree(b)
        .resize(96, 80, 1.0)
        .insert_scene(
            RetainedParent::mask(root),
            None,
            invalid,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .commit();

    assert_eq!(result, Err(RetainedSceneError::InvalidParentBranch(root)));
    assert_eq!(scene.version(), version);
    assert_eq!(
        scene.surface,
        crate::CanvasSurface::from_logical((64, 64), 1.0).unwrap()
    );
    assert!(scene.nodes.contains_key(&b));
    assert!(!scene.nodes.contains_key(&invalid));
    assert_eq!(
        scene.nodes[&root]
            .content
            .order
            .iter()
            .map(|(&key, &id)| (key, id))
            .collect::<Vec<_>>(),
        order
    );
    let NodeKind::Scene { canvas, .. } = &scene.nodes[&a].kind else {
        unreachable!()
    };
    assert!(Rc::ptr_eq(canvas, &original));
}
