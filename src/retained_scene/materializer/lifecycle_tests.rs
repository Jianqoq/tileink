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
