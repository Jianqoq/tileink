use super::*;

#[test]
fn transaction_rejects_non_finite_and_singular_transforms_atomically() {
    let root = RetainedNodeId::for_owner(70_000);
    let child = RetainedNodeId::for_owner(70_001);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            path_leaf(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let version = scene.version();

    for invalid in [
        Affine::new([1.0, 0.0, 0.0, 1.0, f64::NAN, 0.0]),
        Affine::scale_non_uniform(0.0, 1.0),
        Affine::new([1.0, 1.0, 1.0, 1.0 + 1e-9, 0.0, 0.0]),
        Affine::scale(1e-40),
        Affine::translate((f64::MAX, 0.0)),
    ] {
        let mut transaction = scene.transaction();
        transaction
            .set_transform(child, invalid)
            .invalidate_rect(Rect::new(0.0, 0.0, 8.0, 8.0));
        assert_eq!(
            transaction.commit(),
            Err(RetainedSceneError::InvalidTransform)
        );
        assert_eq!(scene.version(), version);
        let NodeKind::Scene { transform, .. } = &scene.nodes[&child].kind else {
            unreachable!()
        };
        assert_eq!(*transform, Affine::IDENTITY);
    }
}

#[test]
fn bounded_translation_rejects_linear_transform_changes_atomically() {
    let root = RetainedNodeId::for_owner(70_016);
    let child = RetainedNodeId::for_owner(70_017);
    let damage = Rect::new(0.0, 0.0, 96.0, 64.0);
    let mut scene = RetainedScene::new(96, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_bounded_scene(
            RetainedParent::content(root),
            None,
            child,
            path_leaf(),
            Affine::IDENTITY,
            damage,
        )
        .commit()
        .unwrap();
    let version = scene.version();

    let error = scene
        .transaction()
        .set_bounded_translation(child, Affine::scale(2.0), damage)
        .commit()
        .unwrap_err();

    assert!(matches!(error, RetainedSceneError::InvalidTransform));
    assert_eq!(scene.version(), version);
}

#[test]
fn reparent_rejects_cycles() {
    let root = RetainedNodeId::for_owner(1);
    let a = RetainedNodeId::for_owner(2);
    let b = RetainedNodeId::for_owner(3);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    transaction
        .insert_group(RetainedParent::content(root), None, a)
        .insert_group(RetainedParent::content(a), None, b);
    transaction.commit().unwrap();
    let mut transaction = scene.transaction();
    transaction.reparent(a, RetainedParent::content(b), None);
    assert_eq!(transaction.commit(), Err(RetainedSceneError::Cycle(a)));
}

#[test]
fn mask_branch_is_only_valid_for_masks() {
    let root = RetainedNodeId::for_owner(1);
    let group = RetainedNodeId::for_owner(2);
    let child = RetainedNodeId::for_owner(3);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    transaction
        .insert_group(RetainedParent::content(root), None, group)
        .insert_scene(
            RetainedParent::mask(group),
            None,
            child,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        );
    assert_eq!(
        transaction.commit(),
        Err(RetainedSceneError::InvalidParentBranch(group))
    );
}

#[test]
fn layer_geometry_rejects_non_finite_coordinates_atomically() {
    let root = RetainedNodeId::for_owner(20);
    let layer = RetainedNodeId::for_owner(21);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let descriptor = RetainedLayerDescriptor::Opacity {
        path: Rect::new(0.0, 0.0, f64::NAN, 16.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity: 0.5,
    };

    assert_eq!(
        scene
            .transaction()
            .insert_layer(RetainedParent::content(root), None, layer, descriptor)
            .commit(),
        Err(RetainedSceneError::InvalidPosition)
    );
    assert_eq!(scene.version(), SceneVersion::INITIAL);
    assert!(!scene.nodes.contains_key(&layer));

    let descriptor = RetainedLayerDescriptor::ClipSdf {
        sdf: Sdf::Circle(crate::shared::sdf::circle::Circle {
            center: Point::new(f64::NAN, 8.0),
            radius: 4.0,
        }),
        transform: Affine::IDENTITY,
    };
    assert_eq!(
        scene
            .transaction()
            .insert_layer(RetainedParent::content(root), None, layer, descriptor)
            .commit(),
        Err(RetainedSceneError::InvalidPosition)
    );
    assert_eq!(scene.version(), SceneVersion::INITIAL);
    assert!(!scene.nodes.contains_key(&layer));

    let descriptor = RetainedLayerDescriptor::ClipSdf {
        sdf: Sdf::Rect(crate::SdfRect {
            start: Point::new(0.0, 0.0),
            end: Point::new(16.0, 16.0),
            radius: crate::Radius::ZERO,
        }),
        transform: Affine::scale_non_uniform(0.0, 1.0),
    };
    assert_eq!(
        scene
            .transaction()
            .insert_layer(RetainedParent::content(root), None, layer, descriptor)
            .commit(),
        Err(RetainedSceneError::InvalidTransform)
    );
    assert_eq!(scene.version(), SceneVersion::INITIAL);
    assert!(!scene.nodes.contains_key(&layer));
}

#[test]
fn gpu_transform_validation_uses_the_physical_surface_scale() {
    let root = RetainedNodeId::for_owner(70_022);
    let child = RetainedNodeId::for_owner(70_023);
    let layer = RetainedNodeId::for_owner(70_024);
    let mut scene = RetainedScene::new(64, 64, 2.0, root).unwrap();
    let canvas = Rc::new(Canvas::new(32, 32, 2.0));
    let overflow = Affine::translate((f64::from(f32::MAX) * 0.75, 0.0));
    assert_eq!(
        scene
            .transaction()
            .insert_scene(RetainedParent::content(root), None, child, canvas, overflow,)
            .commit(),
        Err(RetainedSceneError::InvalidTransform)
    );
    assert_eq!(scene.version(), SceneVersion::INITIAL);
    let descriptor = |transform| RetainedLayerDescriptor::ClipSdf {
        sdf: Sdf::Rect(crate::SdfRect {
            start: Point::ZERO,
            end: Point::new(16.0, 16.0),
            radius: crate::Radius::ZERO,
        }),
        transform,
    };
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            descriptor(Affine::scale(0.0001)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let version = scene.version();
    assert_eq!(
        scene
            .transaction()
            .update_layer(layer, descriptor(overflow))
            .commit(),
        Err(RetainedSceneError::InvalidTransform)
    );
    assert_eq!(scene.version(), version);
    assert!(!update_materializer(&mut materializer, &scene));
    let surface = crate::CanvasSurface::from_physical((128, 128), 4.0).unwrap();
    let current_only = Affine::translate((f64::from(f32::MAX) * 0.3, 0.0));
    scene
        .transaction()
        .update_layer(layer, descriptor(current_only))
        .commit()
        .unwrap();
    let old_surface = scene.surface();
    let version = scene.version();
    assert_eq!(
        scene.transaction().set_surface(surface).commit(),
        Err(RetainedSceneError::InvalidTransform)
    );
    assert_eq!(scene.surface(), old_surface);
    assert_eq!(scene.version(), version);
    scene
        .transaction()
        .update_layer(layer, descriptor(Affine::scale(0.0001)))
        .set_surface(surface)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
}
