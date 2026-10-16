use super::*;

#[test]
fn scene_content_replacement_refreshes_embedded_backdrop_index() {
    let root = RetainedNodeId::for_owner(60_000);
    let child = RetainedNodeId::for_owner(60_001);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            Affine::translate((8.0, 12.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    assert!(materializer.dependency_free);
    assert!(!materializer.nonlocal_dependencies.contains(&child));
    assert!(!materializer.surface_dependent_plans.contains(&child));

    scene
        .transaction()
        .replace_scene(child, backdrop_leaf())
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(!materializer.dependency_free);
    assert!(materializer.nonlocal_dependencies.contains(&child));
    assert!(materializer.surface_dependent_plans.contains(&child));
    assert_eq!(
        materializer.chunks[&child].backdrop_dependencies[0].output,
        Bounds::new(8, 12, 24, 28)
    );

    scene
        .transaction()
        .replace_scene(child, leaf(Color::BLACK))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(materializer.dependency_free);
    assert!(!materializer.nonlocal_dependencies.contains(&child));
    assert!(!materializer.surface_dependent_plans.contains(&child));
}

#[test]
fn same_plan_content_revision_keeps_embedded_backdrop_index() {
    let root = RetainedNodeId::for_owner(60_010);
    let child = RetainedNodeId::for_owner(60_011);
    let backdrop_with_color = |color| {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_backdrop_layer(
            Filter::Opacity(0.5),
            Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO),
        );
        canvas.pop_layer();
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, color);
        Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            backdrop_with_color(Color::WHITE),
            Affine::translate((8.0, 12.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let plan = materializer.chunks[&child]
        .canvas
        .execution_plan_fingerprint();

    scene
        .transaction()
        .replace_scene(child, backdrop_with_color(Color::BLACK))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_eq!(
        materializer.chunks[&child]
            .canvas
            .execution_plan_fingerprint(),
        plan
    );
    let rebuilt = PersistentSceneMaterializer::new(&scene);
    assert_eq!(
        materializer.nonlocal_dependencies,
        rebuilt.nonlocal_dependencies
    );
    assert_eq!(
        materializer.surface_dependent_plans,
        rebuilt.surface_dependent_plans
    );
    assert_eq!(
        materializer.chunks[&child].backdrop_dependencies[0].output,
        rebuilt.chunks[&child].backdrop_dependencies[0].output,
    );
}

#[test]
fn expanding_nested_clip_patches_newly_visible_raw_spatial_candidates() {
    let root = RetainedNodeId::for_owner(82_500);
    let outer = RetainedNodeId::for_owner(82_501);
    let inner = RetainedNodeId::for_owner(82_502);
    let child = RetainedNodeId::for_owner(82_503);
    let far = RetainedNodeId::for_owner(82_504);
    let inserted = RetainedNodeId::for_owner(82_505);
    let clip = |x0, x1| RetainedLayerDescriptor::ClipSdf {
        sdf: Sdf::Rect(crate::SdfRect {
            start: Point::new(x0, 0.0),
            end: Point::new(x1, 16.0),
            radius: Radius::ZERO,
        }),
        transform: Affine::IDENTITY,
    };
    let mut scene = RetainedScene::new(80, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(RetainedParent::content(root), None, outer, clip(0.0, 16.0))
        .insert_layer(
            RetainedParent::content(outer),
            None,
            inner,
            clip(20.0, 40.0),
        )
        .insert_scene(
            RetainedParent::content(inner),
            None,
            child,
            leaf(Color::WHITE),
            Affine::translate((20.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(outer),
            None,
            far,
            leaf(Color::WHITE),
            Affine::translate((48.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let base = materializer.canvas.persistent_frame.clone().unwrap();
    assert!(base.node_state(child).unwrap().bounds.is_empty());
    assert!(base.node_state(inner).unwrap().bounds.is_empty());
    assert!(base.node_state(far).unwrap().bounds.is_empty());

    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(outer),
            None,
            inserted,
            leaf(Color::WHITE),
            Affine::translate((32.0, 0.0)),
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let inserted_frame = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(Rc::ptr_eq(&base.nodes, &inserted_frame.nodes));
    assert!(
        inserted_frame
            .node_state(inserted)
            .unwrap()
            .bounds
            .is_empty()
    );

    scene
        .transaction()
        .update_layer(outer, clip(0.0, 40.0))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let expanded = materializer.canvas.persistent_frame.clone().unwrap();
    assert!(Rc::ptr_eq(&base.nodes, &expanded.nodes));
    assert!(!expanded.node_state(child).unwrap().bounds.is_empty());
    assert!(!expanded.node_state(inner).unwrap().bounds.is_empty());
    assert!(!expanded.node_state(inserted).unwrap().bounds.is_empty());
    assert!(expanded.node_state(far).unwrap().bounds.is_empty());
    let patched = expanded
        .delta
        .as_ref()
        .unwrap()
        .patches
        .iter()
        .map(|patch| patch.new.unwrap().id)
        .collect::<HashSet<_>>();
    assert_eq!(
        patched,
        [outer, inner, child, inserted].into_iter().collect()
    );

    scene
        .transaction()
        .update_layer(outer, clip(0.0, 16.0))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let shrunk = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(Rc::ptr_eq(&base.nodes, &shrunk.nodes));
    assert!(shrunk.node_state(child).unwrap().bounds.is_empty());
    assert!(shrunk.node_state(inner).unwrap().bounds.is_empty());
    assert!(shrunk.node_state(inserted).unwrap().bounds.is_empty());
    assert!(shrunk.node_state(far).unwrap().bounds.is_empty());

    for index in 0..256 {
        scene
            .transaction()
            .update_layer(outer, clip(0.0, if index % 2 == 0 { 40.0 } else { 16.0 }))
            .commit()
            .unwrap();
        assert!(update_materializer(&mut materializer, &scene));
    }
    let repeated = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(Rc::ptr_eq(&base.nodes, &repeated.nodes));
    assert_eq!(repeated.delta.as_ref().unwrap().depth, 1);
    assert!(repeated.delta.as_ref().unwrap().previous.is_none());
}

#[test]
fn clip_bounds_update_inside_filter_uses_full_frame_fallback() {
    let root = RetainedNodeId::for_owner(82_510);
    let filter = RetainedNodeId::for_owner(82_511);
    let clip = RetainedNodeId::for_owner(82_512);
    let child = RetainedNodeId::for_owner(82_513);
    let descriptor = |width| RetainedLayerDescriptor::ClipSdf {
        sdf: Sdf::Rect(crate::SdfRect {
            start: Point::ZERO,
            end: Point::new(width, 16.0),
            radius: Radius::ZERO,
        }),
        transform: Affine::IDENTITY,
    };
    let mut scene = RetainedScene::new(64, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            filter,
            RetainedLayerDescriptor::Filter {
                filter: Filter::Opacity(0.5),
                sample_region: Region::rect(Rect::new(0.0, 0.0, 64.0, 16.0), Radius::ZERO),
            },
        )
        .insert_layer(
            RetainedParent::content(filter),
            None,
            clip,
            descriptor(16.0),
        )
        .insert_scene(
            RetainedParent::content(clip),
            None,
            child,
            leaf(Color::WHITE),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let base = materializer.canvas.persistent_frame.clone().unwrap();

    scene
        .transaction()
        .update_layer(clip, descriptor(32.0))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let updated = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(!Rc::ptr_eq(&base.nodes, &updated.nodes));
    assert!(!updated.node_state(child).unwrap().bounds.is_empty());
}
