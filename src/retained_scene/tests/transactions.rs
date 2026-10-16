use super::*;

#[test]
fn insert_before_rejects_invalid_siblings_even_when_the_branch_is_empty() {
    let root = RetainedNodeId::for_owner(83_000);
    let target = RetainedNodeId::for_owner(83_001);
    let child = RetainedNodeId::for_owner(83_002);
    let missing = RetainedNodeId::for_owner(83_003);
    for branch in 0..3 {
        for before in [missing, child] {
            let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
            let parent = match branch {
                0 => RetainedParent::content(root),
                1 => {
                    scene
                        .transaction()
                        .insert_group(RetainedParent::content(root), None, target)
                        .commit()
                        .unwrap();
                    RetainedParent::content(target)
                }
                _ => {
                    scene
                        .transaction()
                        .insert_layer(
                            RetainedParent::content(root),
                            None,
                            target,
                            RetainedLayerDescriptor::Mask(Mask {
                                region: Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO),
                                kind: crate::MaskKind::Alpha,
                            }),
                        )
                        .commit()
                        .unwrap();
                    RetainedParent::mask(target)
                }
            };
            let version = scene.version();
            let nodes = scene.nodes.len();
            let result = scene
                .transaction()
                .resize(96, 80, 1.0)
                .insert_group(parent, Some(before), child)
                .commit();
            assert_eq!(
                result,
                Err(RetainedSceneError::InvalidSibling(before)),
                "branch={branch}"
            );
            assert_eq!(scene.version(), version);
            assert_eq!(scene.nodes.len(), nodes);
            assert_eq!(scene.logical_size(), (64.0, 64.0));
            assert!(!scene.nodes.contains_key(&child));
            assert!(
                scene
                    .changes_since(version)
                    .unwrap()
                    .changed_nodes
                    .is_empty()
            );
            // Rejection must not poison subsequent valid inserts or their order keys.
            scene
                .transaction()
                .insert_group(parent, None, child)
                .commit()
                .unwrap();
            let node = &scene.nodes[&parent.node];
            let children = match parent.branch {
                RetainedChildBranch::Content => &node.content,
                RetainedChildBranch::Mask => &node.mask,
            };
            assert_eq!(
                children.order.values().copied().collect::<Vec<_>>(),
                [child]
            );
            assert_eq!(children.keys.len(), 1);
        }
    }
}

#[test]
fn scene_leaves_reject_retained_children_in_both_branches() {
    let root = RetainedNodeId::for_owner(83_030);
    let leaf_id = RetainedNodeId::for_owner(83_031);
    let child = RetainedNodeId::for_owner(83_032);
    let inserted = RetainedNodeId::for_owner(83_033);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            leaf_id,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_group(RetainedParent::content(root), None, child)
        .commit()
        .unwrap();
    let version = scene.version();
    let order = scene.nodes[&root].content.clone();
    for parent in [
        RetainedParent::content(leaf_id),
        RetainedParent::mask(leaf_id),
    ] {
        assert_eq!(
            scene
                .transaction()
                .insert_group(parent, None, inserted)
                .commit(),
            Err(RetainedSceneError::InvalidParentBranch(leaf_id))
        );
        assert_eq!(
            scene.transaction().reparent(child, parent, None).commit(),
            Err(RetainedSceneError::InvalidParentBranch(leaf_id))
        );
        assert_eq!(scene.version(), version);
        assert!(!scene.nodes.contains_key(&inserted));
        assert_eq!(
            scene.nodes[&child].parent,
            Some(RetainedParent::content(root))
        );
        assert_eq!(scene.nodes[&root].content.order, order.order);
        assert_eq!(scene.nodes[&root].content.keys, order.keys);
        assert!(scene.nodes[&leaf_id].content.is_empty());
        assert!(scene.nodes[&leaf_id].mask.is_empty());
    }
}

#[test]
fn reparent_invalid_mask_branch_reports_the_actual_target_and_preserves_order() {
    let root = RetainedNodeId::for_owner(83_010);
    let group = RetainedNodeId::for_owner(83_011);
    let child = RetainedNodeId::for_owner(83_012);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_group(RetainedParent::content(root), None, group)
        .insert_group(RetainedParent::content(root), None, child)
        .commit()
        .unwrap();
    for target in [root, group] {
        let version = scene.version();
        let order = scene.nodes[&root].content.clone();
        let result = scene
            .transaction()
            .reparent(child, RetainedParent::mask(target), None)
            .commit();
        assert_eq!(result, Err(RetainedSceneError::InvalidParentBranch(target)));
        assert_eq!(scene.version(), version);
        assert_eq!(
            scene.nodes[&child].parent,
            Some(RetainedParent::content(root))
        );
        assert_eq!(scene.nodes[&root].content.order, order.order);
        assert_eq!(scene.nodes[&root].content.keys, order.keys);
    }
}

#[test]
fn move_before_self_requires_an_existing_node() {
    let root = RetainedNodeId::for_owner(83_020);
    let child = RetainedNodeId::for_owner(83_021);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    assert_eq!(
        scene.transaction().move_before(child, child).commit(),
        Err(RetainedSceneError::MissingNode(child))
    );
    assert_eq!(scene.version(), SceneVersion::INITIAL);
    scene
        .transaction()
        .insert_group(RetainedParent::content(root), None, child)
        .commit()
        .unwrap();
    let version = scene.version();
    for id in [root, child] {
        assert_eq!(
            scene.transaction().move_before(id, id).commit(),
            Ok(version)
        );
    }
}
