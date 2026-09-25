use super::*;

#[test]
fn repeated_reorder_with_groups_preserves_painter_order() {
    for nested in [false, true] {
        let root = RetainedNodeId::for_owner(82_000);
        let empty = RetainedNodeId::for_owner(82_001);
        let group = RetainedNodeId::for_owner(82_002);
        let first = RetainedNodeId::for_owner(82_003);
        let second = RetainedNodeId::for_owner(82_004);
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        let parent = if nested {
            transaction.insert_group(RetainedParent::content(root), None, group);
            group
        } else {
            root
        };
        transaction.insert_group(RetainedParent::content(parent), None, empty);
        transaction
            .insert_scene(
                RetainedParent::content(parent),
                None,
                first,
                leaf(Color::WHITE),
                Affine::IDENTITY,
            )
            .insert_scene(
                RetainedParent::content(parent),
                None,
                second,
                leaf(Color::BLACK),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        let mut materializer = PersistentSceneMaterializer::new(&scene);
        let bounds = materializer
            .canvas
            .persistent_frame
            .as_ref()
            .unwrap()
            .node_state(first)
            .unwrap()
            .bounds;

        // Reuse the materializer across enough edits to exhaust the sibling order-key gap.
        // Structural groups have no draw chunk or frame node, even when their order changes.
        let mut group_rebalanced = false;
        for frame_number in 0..600 {
            let order = if frame_number % 2 == 0 {
                [first, second]
            } else {
                [second, first]
            };
            scene
                .transaction()
                .move_before(order[0], order[1])
                .commit()
                .unwrap();
            let changes = scene.changes_since(materializer.version()).unwrap();
            group_rebalanced |= changes.changed_nodes.contains(&empty);
            materializer.update(&scene, Some(changes));
            assert_eq!(
                root_retained_commands(&materializer),
                order,
                "nested={nested}, frame={frame_number}"
            );
            let frame = materializer.canvas.persistent_frame.as_ref().unwrap();
            for id in [root, empty, group] {
                assert!(frame.node_state(id).is_none());
            }
            for id in [first, second] {
                assert_eq!(frame.node_state(id).unwrap().bounds, bounds);
            }
        }
        assert!(
            group_rebalanced,
            "the unchanged group must acquire a new sibling order key"
        );
    }
}
