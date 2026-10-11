use super::*;

#[test]
fn painter_dirty_ranges_drop_removed_suffix_after_draw_arena_compaction() {
    let root = RetainedNodeId::for_owner(62_075);
    let child = RetainedNodeId::for_owner(62_076);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let current_len = materializer.canvas.stable_batch_ids.as_ref().unwrap().len();

    // Arena compaction shortens the current draw arrays before painter metadata is rebuilt. Model
    // the previous high-water metadata that used to produce `current_len..old_len` as an upload.
    Rc::make_mut(&mut materializer.canvas)
        .painter_keys
        .as_mut()
        .unwrap()
        .extend(std::iter::repeat_n(PainterKey::inactive(), 8));
    materializer.rebuild_painter_metadata(&scene);

    let canvas = materializer.canvas();
    let batch_len = canvas.stable_batch_ids.as_ref().unwrap().len();
    assert_eq!(batch_len, current_len);
    assert!(
        canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .painter
            .iter()
            .all(|range| range.end <= batch_len)
    );
}

#[test]
fn painter_rebuild_uploads_batch_only_metadata_changes() {
    let root = RetainedNodeId::for_owner(62_073);
    let child = RetainedNodeId::for_owner(62_074);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let physical = materializer.canvas.stable_batch_ids.as_ref().unwrap()[..]
        .iter()
        .position(|batch| *batch != u32::MAX)
        .unwrap();
    Rc::make_mut(&mut materializer.canvas)
        .stable_batch_ids
        .as_mut()
        .unwrap()[physical] = u32::MAX;

    materializer.rebuild_painter_metadata(&scene);

    let canvas = materializer.canvas();
    assert_ne!(
        canvas.stable_batch_ids.as_ref().unwrap()[physical],
        u32::MAX
    );
    assert!(
        canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .painter
            .iter()
            .any(|range| range.contains(&physical)),
        "stable batch IDs share the painter metadata buffer and require the same upload tracking"
    );
}
