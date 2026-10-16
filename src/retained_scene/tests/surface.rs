use super::*;

#[test]
fn surface_resize_reuses_chunks_and_refreshes_path_tile_bounds() {
    let root = RetainedNodeId::for_owner(62_000);
    let clip = RetainedNodeId::for_owner(62_002);
    let child = RetainedNodeId::for_owner(62_001);
    let mut path_leaf = Canvas::new(96, 32, 1.0);
    path_leaf.push_path(
        Rect::new(8.0, 4.0, 88.0, 28.0).to_path(0.1),
        Color::WHITE,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    let mut scene = RetainedScene::new(32, 32, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            clip,
            RetainedLayerDescriptor::ClipPath {
                path: Rect::new(0.0, 0.0, 96.0, 32.0).to_path(0.1),
                transform: Affine::IDENTITY,
                rule: FillRule::NonZero,
                tolerance: 0.1,
            },
        )
        .insert_scene(
            RetainedParent::content(clip),
            None,
            child,
            Rc::new(path_leaf),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let chunk_canvas = std::ptr::from_ref(&materializer.chunks[&child].canvas);
    assert_eq!(
        materializer.chunks[&child].canvas.path_records[0].tile_x1,
        2
    );

    scene.transaction().resize(96, 32, 1.0).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    assert_eq!(
        std::ptr::from_ref(&materializer.chunks[&child].canvas),
        chunk_canvas
    );
    assert_eq!(
        materializer.chunks[&child].canvas.path_records[0].tile_x1,
        6
    );
    let canvas = materializer.canvas();
    assert_eq!(canvas.logical_size(), (96.0, 32.0));
    let changes = canvas.buffer_changes.as_ref().unwrap();
    assert_eq!(changes.chunks_rebuilt, 0);
    assert!(!changes.full_scene_sync);
}

#[test]
fn surface_resize_preserves_painter_keys_when_backdrop_rebuilds_the_plan() {
    let root = RetainedNodeId::for_owner(62_003);
    let clip = RetainedNodeId::for_owner(62_004);
    let background = RetainedNodeId::for_owner(62_005);
    let panel = RetainedNodeId::for_owner(62_006);
    let mut background_canvas = Canvas::new(96, 32, 1.0);
    for y in [2.0, 14.0] {
        background_canvas.push_path(
            Rect::new(0.0, y, 96.0, y + 8.0).to_path(0.1),
            Color::WHITE,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.1,
        );
    }
    let mut panel_canvas = Canvas::new(16, 16, 1.0);
    panel_canvas.push_backdrop_layer(
        Filter::Opacity(0.5),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO),
    );
    panel_canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, Color::WHITE);
    panel_canvas.pop_layer();
    let mut scene = RetainedScene::new(32, 32, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            clip,
            RetainedLayerDescriptor::ClipPath {
                path: Rect::new(0.0, 0.0, 96.0, 32.0).to_path(0.1),
                transform: Affine::IDENTITY,
                rule: FillRule::NonZero,
                tolerance: 0.1,
            },
        )
        .insert_scene(
            RetainedParent::content(clip),
            None,
            background,
            Rc::new(background_canvas),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(clip),
            None,
            panel,
            Rc::new(panel_canvas),
            Affine::translate((8.0, 8.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let before = materializer.canvas.painter.keys.clone();
    assert_eq!(
        before.iter().filter(|key| key.path[0] != u128::MAX).count(),
        3
    );

    scene.transaction().resize(96, 32, 1.0).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    // Surface remapping dirties live draw records but does not change their painter ownership.
    // Clearing those keys made the resize-only tile index omit every unchanged vector draw.
    assert_eq!(materializer.canvas.painter.keys, before);
    assert!(
        materializer
            .canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .plan_fragments_rebuilt
            > 0
    );
}

#[test]
fn surface_resize_discards_removed_path_chunks_before_resizing_scan_allocations() {
    let path_grid = |count: usize, width: f64| {
        let mut canvas = Canvas::new(512, 32, 1.0);
        for index in 0..count {
            let y = (index % 16) as f64 * 2.0;
            canvas.push_path(
                Rect::new(0.0, y, width, y + 1.0).to_path(0.1),
                Color::WHITE,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
            );
        }
        Rc::new(canvas)
    };
    let root = RetainedNodeId::for_owner(62_010);
    let fragmented = RetainedNodeId::for_owner(62_011);
    let survivor = RetainedNodeId::for_owner(62_012);
    let removed_during_resize = RetainedNodeId::for_owner(62_013);
    let mut scene = RetainedScene::new(32, 32, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            fragmented,
            path_grid(64, 16.0),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            survivor,
            path_grid(1, 16.0),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            removed_during_resize,
            path_grid(24, 512.0),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);

    scene
        .transaction()
        .remove_subtree(fragmented)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let compactions = materializer.arena_compactions();

    scene
        .transaction()
        .resize(512, 32, 1.0)
        .remove_subtree(removed_during_resize)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    // Removed previous-frame geometry must never be expanded to the new viewport. Besides wasted
    // work, doing so can compact the shared scan arenas immediately before the allocation is
    // deleted, which was the root cause of responsive SVG chunks disappearing during resize.
    assert_eq!(materializer.arena_compactions(), compactions);
    assert!(materializer.chunks.contains_key(&survivor));
    assert!(!materializer.chunks.contains_key(&removed_during_resize));
}

#[test]
fn same_scale_resize_and_removal_restore_live_spatial_nodes_after_resize() {
    let root = RetainedNodeId::for_owner(62_050);
    let removed = RetainedNodeId::for_owner(62_051);
    let retained = RetainedNodeId::for_owner(62_052);
    let leaf = |color| {
        let mut canvas = Canvas::new(16, 16, 2.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, color);
        Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(64, 64, 2.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            removed,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            retained,
            leaf(Color::BLACK),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let previous_spatial_tiles_size = materializer.spatial_tiles_size;

    scene
        .transaction()
        .resize(96, 64, 2.0)
        .remove_subtree(removed)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    assert!(!materializer.chunks.contains_key(&removed));
    assert!(materializer.chunks.contains_key(&retained));
    assert!(materializer.surface_metadata_stale);
    assert_eq!(materializer.spatial_tiles_size, previous_spatial_tiles_size);
    assert!(
        materializer
            .canvas
            .persistent_frame
            .as_ref()
            .unwrap()
            .node_state(removed)
            .is_some(),
        "full redraw keeps the previous immutable damage metadata until incremental work resumes"
    );

    scene
        .transaction()
        .set_transform(retained, Affine::translate((32.0, 0.0)))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    assert_eq!(
        materializer.spatial_tiles_size,
        (
            materializer.canvas.width_in_tiles(),
            materializer.canvas.height_in_tiles(),
        )
    );
    assert!(
        materializer
            .canvas
            .persistent_frame
            .as_ref()
            .unwrap()
            .node_state(removed)
            .is_none()
    );
    assert!(!materializer.surface_metadata_stale);
}
