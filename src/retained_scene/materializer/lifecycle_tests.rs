use super::*;

#[test]
fn raster_invalidation_publishes_empty_upload_delta() {
    let root = RetainedNodeId::for_owner(1);
    let child = RetainedNodeId::for_owner(2);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut content = Canvas::new(64, 64, 1.0);
    content.push_rect(
        peniko::kurbo::Rect::new(0.0, 0.0, 8.0, 8.0),
        crate::Radius::ZERO,
        peniko::Color::WHITE,
    );
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            Rc::new(content),
            peniko::kurbo::Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    for full in [false, true, false] {
        let mut transaction = scene.transaction();
        if full {
            transaction.invalidate_all();
        } else {
            transaction.invalidate_rect(peniko::kurbo::Rect::new(0.0, 0.0, 4.0, 4.0));
        }
        transaction.commit().unwrap();
        assert!(!materializer.update(&scene, scene.changes_since(materializer.version())));
        let canvas = materializer.canvas();
        let changes = canvas
            .buffer_changes
            .as_ref()
            .expect("raster-only damage must not mean full upload");
        assert!(changes.draws.is_empty() && changes.paths.is_empty() && changes.sdfs.is_empty());
        assert!(changes.plan_structure_reused);
        assert!(!changes.full_scene_sync);
    }
}

#[test]
fn resized_node_reencode_matches_fresh_path_allocations() {
    use peniko::kurbo::{Affine, Rect, Shape};

    let root = RetainedNodeId::for_owner(101);
    let child = RetainedNodeId::for_owner(102);
    let mut scene = RetainedScene::new(32, 32, 1.0, root).unwrap();
    let mut content = Canvas::new(96, 32, 1.0);
    content.push_path(
        Rect::new(8.0, 4.0, 88.0, 28.0).to_path(0.1),
        peniko::Color::WHITE,
        Affine::IDENTITY,
        crate::FillRule::NonZero,
        0.1,
    );
    let content = Rc::new(content);
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            Rc::clone(&content),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let old_node = &scene.nodes[&child];
    let mut reused = PersistentSceneMaterializer::encode_node(&scene, old_node);
    scene
        .transaction()
        .resize(96, 32, 1.0)
        .replace_scene(child, content)
        .commit()
        .unwrap();
    let node = &scene.nodes[&child];
    PersistentSceneMaterializer::encode_node_into(&scene, node, &mut reused);
    let fresh = PersistentSceneMaterializer::encode_node(&scene, node);
    assert_eq!(reused.logical_size(), fresh.logical_size());
    assert_eq!(reused.backdrop_pool_capacity, fresh.backdrop_pool_capacity);
    assert_eq!(reused.tile_cnt, fresh.tile_cnt);
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&reused.path_records),
        bytemuck::cast_slice::<_, u8>(&fresh.path_records)
    );
    assert_eq!(
        bytemuck::cast_slice::<_, u8>(&reused.draw_records),
        bytemuck::cast_slice::<_, u8>(&fresh.draw_records)
    );
}
