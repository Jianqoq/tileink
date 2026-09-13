use super::*;

#[test]
fn root_layer_insertion_survives_a_journal_gap() {
    // The topology guard relies on reconciliation classifying a layer insertion even
    // after its original journal entry expires. Keep a published frame alive, as a
    // renderer can, and compare the reconstructed plan and node state with a fresh one.
    let root = RetainedNodeId::for_owner(72_000);
    let base = RetainedNodeId::for_owner(72_001);
    let layer = RetainedNodeId::for_owner(72_002);
    let child = RetainedNodeId::for_owner(72_003);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let published = materializer.canvas.persistent_frame.clone().unwrap();
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
            leaf(Color::BLACK),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    for _ in 0..=JOURNAL_CAPACITY {
        scene
            .transaction()
            .invalidate_rect(Rect::new(0.0, 0.0, 1.0, 1.0))
            .commit()
            .unwrap();
    }
    assert!(scene.changes_since(materializer.version()).is_none());
    assert!(update_materializer(&mut materializer, &scene));
    assert!(
        materializer
            .canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .full_scene_sync
    );
    let fresh = PersistentSceneMaterializer::new(&scene);
    assert_eq!(
        root_retained_commands(&materializer),
        root_retained_commands(&fresh)
    );
    let updated = materializer.canvas.persistent_frame.as_ref().unwrap();
    let expected = fresh.canvas.persistent_frame.as_ref().unwrap();
    for id in [base, layer, child] {
        assert_eq!(
            updated.node_state(id).unwrap().bounds,
            expected.node_state(id).unwrap().bounds
        );
    }
    assert!(published.node_state(layer).is_none());
    assert!(published.node_state(child).is_none());
    let plan_shape = |state: &PersistentSceneMaterializer| {
        let plan = state
            .canvas
            .compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        for id in [base, child] {
            let physical_draws = state.node_physical_draws(id).collect::<Vec<_>>();
            assert!(!physical_draws.is_empty());
            assert!(
                physical_draws
                    .iter()
                    .all(|physical| plan.ops.iter().any(|op| {
                        matches!(op, crate::shared::execution::ExecOp::DrawBatch { draws, .. }
                    if draws.contains(physical))
                    }))
            );
        }
        plan.ops
            .iter()
            .map(|op| match op {
                crate::shared::execution::ExecOp::DrawBatch {
                    draws, layer_stack, ..
                } => (std::mem::discriminant(op), draws.len(), layer_stack.len()),
                _ => (std::mem::discriminant(op), 0, 0),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(plan_shape(&materializer), plan_shape(&fresh));
    assert_eq!(
        plan_shape(&materializer)
            .iter()
            .filter(|(_, draws, _)| *draws > 0)
            .count(),
        2
    );
}
