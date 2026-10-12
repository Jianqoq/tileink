use super::*;

#[test]
fn resize_with_layer_updates_defers_full_frame_and_spatial_rebuild() {
    let root = RetainedNodeId::for_owner(62_100);
    let clip = RetainedNodeId::for_owner(62_101);
    let child = RetainedNodeId::for_owner(62_102);
    let descriptor = |width| RetainedLayerDescriptor::ClipSdf {
        sdf: Sdf::Rect(crate::SdfRect {
            start: Point::ZERO,
            end: Point::new(width, 64.0),
            radius: Radius::ZERO,
        }),
        transform: Affine::IDENTITY,
    };
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(RetainedParent::content(root), None, clip, descriptor(64.0))
        .insert_scene(
            RetainedParent::content(clip),
            None,
            child,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let base_nodes = materializer
        .canvas
        .persistent_frame
        .as_ref()
        .unwrap()
        .nodes
        .clone();

    scene
        .transaction()
        .resize(96, 64, 1.0)
        .update_layer(clip, descriptor(96.0))
        .set_transform(child, Affine::translate((8.0, 0.0)))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let resized = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(Rc::ptr_eq(&resized.nodes, &base_nodes));
    assert!(resized.invalidate_all);
    assert_eq!(resized.logical_size, (96, 64));
    assert_eq!(
        resized.node_state(clip).unwrap().revision,
        NodeRevision::new(scene.nodes[&clip].instance, scene.nodes[&clip].generation),
        "resize frames must publish current revisions for retained surface cache keys"
    );
    assert!(materializer.surface_metadata_stale);

    // The first later incremental edit rebuilds the deferred baseline before applying its delta,
    // so damage and spatial queries observe the exact post-resize bounds.
    scene
        .transaction()
        .set_transform(child, Affine::translate((24.0, 0.0)))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let updated = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(!Rc::ptr_eq(&updated.nodes, &base_nodes));
    assert_eq!(
        updated.node_state(child).unwrap().bounds,
        Bounds::new(24, 0, 40, 16)
    );
    assert!(!materializer.surface_metadata_stale);
}

#[test]
fn resize_with_hierarchy_changes_defers_full_frame_and_spatial_rebuild() {
    let root = RetainedNodeId::for_owner(62_105);
    let retained = RetainedNodeId::for_owner(62_106);
    let removed = RetainedNodeId::for_owner(62_107);
    let inserted = RetainedNodeId::for_owner(62_108);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            retained,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            removed,
            leaf(Color::BLACK),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let base_nodes = materializer
        .canvas
        .persistent_frame
        .as_ref()
        .unwrap()
        .nodes
        .clone();

    scene
        .transaction()
        .resize(96, 64, 1.0)
        .remove_subtree(removed)
        .insert_scene(
            RetainedParent::content(root),
            None,
            inserted,
            leaf(Color::BLACK),
            Affine::translate((48.0, 0.0)),
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let resized = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(
        Rc::ptr_eq(&resized.nodes, &base_nodes),
        "a full redraw must defer hierarchy metadata that damage tracking does not consume"
    );
    assert!(resized.invalidate_all);
    assert_eq!(resized.logical_size, (96, 64));
    assert!(materializer.surface_metadata_stale);

    scene
        .transaction()
        .set_transform(inserted, Affine::translate((64.0, 0.0)))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let updated = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(updated.node_state(removed).is_none());
    assert_eq!(
        updated.node_state(inserted).unwrap().bounds,
        Bounds::new(64, 0, 80, 16)
    );
    assert!(!materializer.surface_metadata_stale);
}

#[test]
fn structural_edit_after_resize_rebuilds_deferred_surface_metadata() {
    let root = RetainedNodeId::for_owner(62_110);
    let existing = RetainedNodeId::for_owner(62_111);
    let inserted = RetainedNodeId::for_owner(62_112);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            existing,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);

    scene.transaction().resize(96, 64, 1.0).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(materializer.surface_metadata_stale);

    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            inserted,
            leaf(Color::BLACK),
            Affine::translate((48.0, 0.0)),
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let frame = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(frame.node_state(existing).is_some());
    assert!(frame.node_state(inserted).is_some());
    assert_eq!(frame.logical_size, (96, 64));
    assert_eq!(
        materializer.spatial_tiles_size,
        (
            materializer.canvas.width_in_tiles(),
            materializer.canvas.height_in_tiles(),
        )
    );
    assert!(!materializer.surface_metadata_stale);
}
