use super::*;

#[test]
fn flat_topology_commands_patch_order_and_reuse_fragment_storage() {
    let root = RetainedNodeId::for_owner(70_005);
    let base = RetainedNodeId::for_owner(70_006);
    let anchor = RetainedNodeId::for_owner(70_007);
    let first = RetainedNodeId::for_owner(70_008);
    let second = RetainedNodeId::for_owner(70_009);
    let recycled = RetainedNodeId::for_owner(70_010);
    let content = leaf(Color::WHITE);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            content.clone(),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            anchor,
            content.clone(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let initial_command_lists = materializer.canvas.command_lists.len();

    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            Some(anchor),
            first,
            content.clone(),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            Some(anchor),
            second,
            content.clone(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_eq!(
        root_retained_commands(&materializer),
        vec![base, first, second, anchor]
    );

    scene
        .transaction()
        .remove_subtree(first)
        .insert_scene(
            RetainedParent::content(root),
            Some(second),
            recycled,
            content.clone(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_eq!(
        root_retained_commands(&materializer),
        vec![base, recycled, second, anchor]
    );
    assert_eq!(
        materializer.canvas.command_lists.len(),
        initial_command_lists + 2,
        "the replacement reuses the removed leaf's command fragment"
    );

    scene
        .transaction()
        .replace_scene(recycled, leaf(Color::BLACK))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
}

#[test]
fn appended_root_layer_fragment_is_visible_in_cached_execution_plan() {
    let root = RetainedNodeId::for_owner(30);
    let base = RetainedNodeId::for_owner(31);
    let layer = RetainedNodeId::for_owner(32);
    let child = RetainedNodeId::for_owner(33);
    let mut scene = RetainedScene::new(64, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            leaf(Color::from_rgb8(20, 40, 220)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let base_batch = materializer.node_batches[&base];
    let base_frame = materializer.canvas.persistent_frame.clone().unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.5,
            },
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            child,
            leaf(Color::from_rgb8(220, 40, 20)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    assert!(update_materializer(&mut materializer, &scene));
    assert_eq!(materializer.node_batches[&base], base_batch);
    let plan = materializer
        .canvas
        .compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    assert_eq!(
        plan.ops
            .iter()
            .filter(|op| matches!(op, crate::shared::execution::ExecOp::DrawBatch { .. }))
            .count(),
        2,
        "patched root plan: {:#?}",
        plan.ops
    );
    let inserted_frame = materializer.canvas.persistent_frame.clone().unwrap();
    assert!(
        Rc::ptr_eq(&base_frame.nodes, &inserted_frame.nodes),
        "root layer insertion must patch the immutable frame instead of collecting every node"
    );
    assert!(inserted_frame.node_state(layer).is_some());
    assert!(inserted_frame.node_state(child).is_some());
    assert_eq!(inserted_frame.delta.as_ref().unwrap().depth, 1);

    scene.transaction().remove_subtree(layer).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let removed_frame = materializer.canvas.persistent_frame.clone().unwrap();
    assert!(Rc::ptr_eq(&base_frame.nodes, &removed_frame.nodes));
    assert!(removed_frame.node_state(layer).is_none());
    assert!(removed_frame.node_state(child).is_none());
    assert_eq!(removed_frame.delta.as_ref().unwrap().depth, 1);
}
