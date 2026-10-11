use super::*;

#[test]
fn transform_only_update_keeps_local_geometry_and_blobs_clean() {
    let root = RetainedNodeId::for_owner(70_010);
    let child = RetainedNodeId::for_owner(70_011);
    let mut scene = RetainedScene::new(96, 96, 1.0, root).unwrap();
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
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let lines: Vec<_> = materializer.chunks[&child]
        .canvas
        .lines
        .iter()
        .map(|line| (line.p0, line.p1, line.path_id))
        .collect();

    let transform = Affine::translate((48.0, 24.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2);
    scene
        .transaction()
        .set_transform(child, transform)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let chunk = &materializer.chunks[&child];
    assert_eq!(
        chunk
            .canvas
            .lines
            .iter()
            .map(|line| (line.p0, line.p1, line.path_id))
            .collect::<Vec<_>>(),
        lines
    );
    let materialized = materializer.canvas();
    let changes = materialized.buffer_changes.as_ref().unwrap();
    assert!(changes.lines.is_empty());
    assert!(changes.brushes.is_empty());
    assert!(changes.sdfs.is_empty());
    assert!(changes.shadows.is_empty());
    assert!(changes.glyphs.is_empty());
    assert!(!changes.paths.is_empty());
    assert!(!changes.draws.is_empty());
    assert_eq!(
        chunk.canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 30,
            y0: 28,
            x1: 42,
            y1: 44,
        }
    );
}

#[test]
fn bounded_translation_keeps_fixed_output_domain_without_tile_duplication() {
    let root = RetainedNodeId::for_owner(70_012);
    let child = RetainedNodeId::for_owner(70_013);
    let damage = Rect::new(16.0, 16.0, 80.0, 48.0);
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
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let initial_raw_bounds = materializer.raw_node_bounds[&child];
    assert_eq!(
        materializer.bounded_node_bounds.get(&child),
        Some(&Bounds::new(16, 16, 80, 48))
    );
    assert!(
        materializer
            .node_tiles
            .iter()
            .all(|nodes| !nodes.contains(&child))
    );

    scene
        .transaction()
        .set_bounded_translation(child, Affine::translate((40.0, 8.0)), damage)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let frame = materializer.canvas().persistent_frame.clone().unwrap();
    let state = frame.node_state(child).unwrap();
    assert_eq!(state.bounds, Bounds::new(16, 16, 80, 48));
    let delta = frame.delta.as_ref().unwrap();
    let patch = delta
        .patches
        .iter()
        .find(|patch| patch.new.is_some_and(|node| node.id == child))
        .unwrap();
    assert_eq!(patch.old.unwrap().bounds, patch.new.unwrap().bounds);
    assert_eq!(patch.damage, Some(Bounds::new(16, 16, 80, 48)));
    assert_eq!(
        materializer.bounded_node_bounds.get(&child),
        Some(&Bounds::new(16, 16, 80, 48))
    );
    assert_eq!(materializer.raw_node_bounds[&child], initial_raw_bounds);
    assert_eq!(
        materializer.bounded_raw_node_bounds.get(&child),
        Some(&Bounds::new(16, 16, 80, 48))
    );
    assert!(
        materializer
            .node_tiles
            .iter()
            .all(|nodes| !nodes.contains(&child))
    );
}

#[test]
fn ordinary_transform_exits_fixed_damage_bounds() {
    let root = RetainedNodeId::for_owner(70_014);
    let child = RetainedNodeId::for_owner(70_015);
    let mut scene = RetainedScene::new(96, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_bounded_scene(
            RetainedParent::content(root),
            None,
            child,
            path_leaf(),
            Affine::IDENTITY,
            Rect::new(0.0, 0.0, 96.0, 64.0),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);

    scene
        .transaction()
        .set_transform(child, Affine::translate((48.0, 24.0)))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let frame = materializer.canvas().persistent_frame.clone().unwrap();
    assert_ne!(
        frame.node_state(child).unwrap().bounds,
        Bounds::new(0, 0, 96, 64)
    );
    assert!(!materializer.bounded_node_bounds.contains_key(&child));
    assert!(
        materializer
            .node_tiles
            .iter()
            .any(|nodes| nodes.contains(&child))
    );
}
