use super::*;

fn assert_consistent(materializer: &PersistentSceneMaterializer) {
    let canvas = &materializer.canvas;
    let keys = &canvas.painter.keys;
    let batches = &canvas.painter.batch_ids;
    let counts = &canvas.painter.batch_counts;
    assert_eq!(keys.len(), canvas.draw_records.len());
    assert_eq!(batches.len(), keys.len());
    let mut expected = vec![0; counts.len()];
    for &batch in batches {
        if batch != u32::MAX {
            expected[batch as usize] += 1;
        }
    }
    assert_eq!(*counts, expected);
}

#[test]
fn empty_painter_metadata_survives_insert_remove_and_dpi_rebuild() {
    let root = RetainedNodeId::for_owner(72_000);
    let child = RetainedNodeId::for_owner(72_001);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    assert_consistent(&materializer);
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            empty_leaf(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_consistent(&materializer);
    scene
        .transaction()
        .replace_scene(child, multi_draw_leaf(5))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_consistent(&materializer);
    scene.transaction().remove_subtree(child).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_consistent(&materializer);
    scene.transaction().resize(96, 80, 2.0).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_consistent(&materializer);
    assert!(materializer.canvas.draw_records.is_empty());
}

fn layered_leaf(color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::ZERO, color);
    canvas.push_filter_layer(
        Filter::Opacity(0.5),
        Region::rect(Rect::new(8.0, 8.0, 24.0, 24.0), Radius::ZERO),
    );
    canvas.push_rect(Rect::new(8.0, 8.0, 24.0, 24.0), Radius::ZERO, color);
    canvas.pop_layer();
    canvas.push_rect(Rect::new(24.0, 24.0, 32.0, 32.0), Radius::ZERO, color);
    Rc::new(canvas)
}

#[test]
fn multi_leaf_painter_patch_preserves_immutable_snapshot_and_layered_membership() {
    let root = RetainedNodeId::for_owner(72_010);
    let ids = [72_011, 72_012, 72_013].map(RetainedNodeId::for_owner);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut tx = scene.transaction();
    for id in ids {
        tx.insert_scene(
            RetainedParent::content(root),
            None,
            id,
            layered_leaf(Color::WHITE),
            Affine::IDENTITY,
        );
    }
    tx.commit().unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let snapshot = materializer.canvas();
    let keys = snapshot.painter.keys.clone();
    let batches = snapshot.painter.batch_ids.clone();
    let counts = snapshot.painter.batch_counts.clone();
    let mut tx = scene.transaction();
    for id in ids {
        tx.replace_scene(id, layered_leaf(Color::BLACK));
    }
    tx.commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_consistent(&materializer);
    assert_eq!(snapshot.painter.keys, keys);
    assert_eq!(snapshot.painter.batch_ids, batches);
    assert_eq!(snapshot.painter.batch_counts, counts);
    let fresh = PersistentSceneMaterializer::new(&scene);
    assert_eq!(materializer.canvas.painter.keys, fresh.canvas.painter.keys);
    assert_eq!(
        materializer.canvas.painter.batch_ids,
        fresh.canvas.painter.batch_ids
    );
    assert_eq!(
        materializer.canvas.painter.batch_counts,
        fresh.canvas.painter.batch_counts
    );
}
