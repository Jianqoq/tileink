use super::*;
use crate::shared::affine::GpuAffine;

#[test]
fn reusing_a_node_id_between_scene_and_layer_refreshes_placement() {
    let root = RetainedNodeId::for_owner(70_018);
    let child = RetainedNodeId::for_owner(70_019);
    let source = path_leaf();
    let opacity_layer = |opacity| RetainedLayerDescriptor::Opacity {
        path: Rect::new(0.0, 0.0, 96.0, 96.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity,
    };
    let mut scene = RetainedScene::new(96, 96, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            std::rc::Rc::clone(&source),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);

    scene
        .transaction()
        .remove_subtree(child)
        .insert_layer(
            RetainedParent::content(root),
            None,
            child,
            opacity_layer(0.25),
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(materializer.chunks[&child].placement.is_none());
    scene
        .transaction()
        .update_layer(child, opacity_layer(0.75))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(materializer.chunks[&child].placement.is_none());
    assert!(matches!(
        &materializer.chunks[&child].canvas.command_lists[0].commands[0],
        Command::Layer {
            layer: crate::shared::layer::Layer::Opacity { opacity: 0.75, .. },
            ..
        }
    ));

    let transform = Affine::translate((24.0, 12.0));
    scene
        .transaction()
        .remove_subtree(child)
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            std::rc::Rc::clone(&source),
            transform,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let chunk = &materializer.chunks[&child];
    let placement = chunk.placement.as_ref().unwrap();
    assert!(Rc::ptr_eq(&placement.source, &source));
    assert_eq!(
        placement.transform_bits,
        transform.as_coeffs().map(f64::to_bits)
    );
    assert_eq!(
        chunk.canvas.draw_records[0].pixel_bounds,
        PixelBounds {
            x0: 28,
            y0: 18,
            x1: 44,
            y1: 30
        }
    );
}

#[test]
fn replaced_scene_geometry_is_used_before_later_transform_only_updates() {
    let root = RetainedNodeId::for_owner(70_016);
    let child = RetainedNodeId::for_owner(70_017);
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
    let mut replacement = Canvas::new(32, 32, 1.0);
    replacement.push_path(
        Rect::new(2.0, 3.0, 28.0, 25.0).to_path(0.1),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene
        .transaction()
        .replace_scene(child, Rc::new(replacement))
        .set_transform(child, Affine::translate((16.0, 12.0)))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(
        !materializer
            .canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .lines
            .is_empty()
    );
    let local = PixelBounds {
        x0: 2,
        y0: 3,
        x1: 28,
        y1: 25,
    };
    assert_eq!(
        materializer.chunks[&child].canvas.draw_records[0].local_pixel_bounds,
        local
    );
    for transform in [
        Affine::translate((24.0, 8.0)),
        Affine::translate((32.0, 16.0)) * Affine::scale(0.5),
    ] {
        scene
            .transaction()
            .set_transform(child, transform)
            .commit()
            .unwrap();
        assert!(update_materializer(&mut materializer, &scene));
        let draw = materializer.chunks[&child].canvas.draw_records[0];
        assert_eq!(draw.local_pixel_bounds, local);
        assert_eq!(
            draw.pixel_bounds,
            GpuAffine::from_logical(transform, 1.0).transform_bounds(local)
        );
        let changes = materializer.canvas.buffer_changes.as_ref().unwrap();
        assert!(changes.lines.is_empty());
        assert!(changes.brushes.is_empty());
        assert!(!changes.draws.is_empty());
    }
}

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
