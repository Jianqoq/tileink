use peniko::{Color, kurbo::Shape};

use super::*;
use crate::{Radius, shared::bounds::PixelBounds};

fn leaf(color: Color) -> Arc<Canvas> {
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, color);
    Arc::new(canvas)
}

fn empty_leaf() -> Arc<Canvas> {
    Arc::new(Canvas::new(16, 16, 1.0))
}

fn backdrop_leaf() -> Arc<Canvas> {
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_backdrop_layer(
        Filter::Opacity(0.5),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO),
    );
    canvas.pop_layer();
    Arc::new(canvas)
}

fn path_leaf() -> Arc<Canvas> {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_path(
        Rect::new(4.0, 6.0, 20.0, 18.0).to_path(0.1),
        Color::WHITE,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    Arc::new(canvas)
}

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
    assert!(materializer.update(&scene));

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
fn transaction_is_atomic_when_a_late_mutation_is_invalid() {
    let root = RetainedNodeId::for_owner(1);
    let child = RetainedNodeId::for_owner(2);
    let missing = RetainedNodeId::for_owner(9);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    transaction
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .reparent(child, RetainedParent::content(missing), None);
    assert_eq!(
        transaction.commit(),
        Err(RetainedSceneError::MissingNode(missing))
    );
    assert_eq!(scene.version(), SceneVersion::INITIAL);
    assert!(!scene.nodes.contains_key(&child));
}

#[test]
fn failed_transaction_restores_order_keys_after_rebalance() {
    let root = RetainedNodeId::for_owner(10);
    let first = RetainedNodeId::for_owner(11);
    let tail = RetainedNodeId::for_owner(12);
    let missing = RetainedNodeId::for_owner(u64::MAX);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            first,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            tail,
            leaf(Color::BLACK),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    let mut owner = 13;
    loop {
        let children = &scene.nodes[&root].content;
        let upper = children.key_of(tail).unwrap();
        let lower = children.order.range(..upper).next_back().unwrap().0;
        if upper - lower == 1 {
            break;
        }
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                Some(tail),
                RetainedNodeId::for_owner(owner),
                leaf(Color::WHITE),
                Affine::translate((0.0, 0.0)),
            )
            .commit()
            .unwrap();
        owner += 1;
    }

    let order_before = scene.nodes[&root].content.order.clone();
    let keys_before = scene.nodes[&root].content.keys.clone();
    let inserted = RetainedNodeId::for_owner(owner);
    let mut transaction = scene.transaction();
    transaction
        .insert_scene(
            RetainedParent::content(root),
            Some(tail),
            inserted,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .reparent(inserted, RetainedParent::content(missing), None);

    assert_eq!(
        transaction.commit(),
        Err(RetainedSceneError::MissingNode(missing))
    );
    assert_eq!(scene.nodes[&root].content.order, order_before);
    assert_eq!(scene.nodes[&root].content.keys, keys_before);

    let mut transaction = scene.transaction();
    transaction
        .move_before(first, tail)
        .reparent(tail, RetainedParent::content(missing), None);
    assert_eq!(
        transaction.commit(),
        Err(RetainedSceneError::MissingNode(missing))
    );
    assert_eq!(scene.nodes[&root].content.order, order_before);
    assert_eq!(scene.nodes[&root].content.keys, keys_before);
}

#[test]
fn content_revision_reuses_chunk_canvas_storage() {
    let root = RetainedNodeId::for_owner(5);
    let child = RetainedNodeId::for_owner(6);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            Affine::translate((8.0, 8.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let chunk_storage = std::ptr::from_ref(&materializer.chunks[&child]);
    let storage = std::ptr::from_ref(&materializer.chunks[&child].canvas);

    scene
        .transaction()
        .replace_scene(child, leaf(Color::BLACK))
        .commit()
        .unwrap();
    assert!(materializer.update(&scene));
    assert_eq!(
        std::ptr::from_ref(&materializer.chunks[&child]),
        chunk_storage
    );
    assert_eq!(
        std::ptr::from_ref(&materializer.chunks[&child].canvas),
        storage
    );
    assert_eq!(materializer.chunks[&child].generation, 1);
}

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
            Arc::new(path_leaf),
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
    assert!(materializer.update(&scene));

    assert_eq!(
        std::ptr::from_ref(&materializer.chunks[&child].canvas),
        chunk_canvas
    );
    assert_eq!(
        materializer.chunks[&child].canvas.path_records[0].tile_x1,
        6
    );
    let canvas = materializer.canvas();
    assert_eq!(canvas.logical_size(), (96, 32));
    let changes = canvas.buffer_changes.as_ref().unwrap();
    assert_eq!(changes.chunks_rebuilt, 0);
    assert!(!changes.full_scene_sync);
}

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
    assert!(materializer.update(&scene));
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
    assert!(materializer.update(&scene));
    assert!(materializer.dependency_free);
    assert!(!materializer.nonlocal_dependencies.contains(&child));
    assert!(!materializer.surface_dependent_plans.contains(&child));
}

#[test]
fn empty_chunk_allocations_grow_and_shrink_without_full_sync() {
    let root = RetainedNodeId::for_owner(7);
    let child = RetainedNodeId::for_owner(8);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            empty_leaf(),
            Affine::translate((4.0, 4.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let chunk = &materializer.chunks[&child];
    assert!(materializer.arenas.draws.range(chunk.draws).is_empty());
    assert!(materializer.arenas.sdfs.range(chunk.sdfs).is_empty());

    scene
        .transaction()
        .replace_scene(child, leaf(Color::WHITE))
        .commit()
        .unwrap();
    assert!(materializer.update(&scene));
    let chunk = &materializer.chunks[&child];
    assert_eq!(materializer.arenas.draws.range(chunk.draws).len(), 1);
    assert!(!materializer.arenas.sdfs.range(chunk.sdfs).is_empty());

    scene
        .transaction()
        .replace_scene(child, empty_leaf())
        .commit()
        .unwrap();
    assert!(materializer.update(&scene));
    let chunk = &materializer.chunks[&child];
    assert!(materializer.arenas.draws.range(chunk.draws).is_empty());
    assert!(materializer.arenas.sdfs.range(chunk.sdfs).is_empty());
}

#[test]
fn undo_log_restores_values_order_removals_and_surface_after_late_failure() {
    let root = RetainedNodeId::for_owner(10);
    let a = RetainedNodeId::for_owner(11);
    let b = RetainedNodeId::for_owner(12);
    let invalid = RetainedNodeId::for_owner(13);
    let original = leaf(Color::WHITE);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            a,
            original.clone(),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            b,
            leaf(Color::BLACK),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    let version = scene.version();
    let order = scene.nodes[&root]
        .content
        .order
        .iter()
        .map(|(&key, &id)| (key, id))
        .collect::<Vec<_>>();

    let result = scene
        .transaction()
        .replace_scene(a, leaf(Color::from_rgb8(20, 80, 220)))
        .move_before(b, a)
        .remove_subtree(b)
        .resize(96, 80, 1.0)
        .insert_scene(
            RetainedParent::mask(root),
            None,
            invalid,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .commit();

    assert_eq!(result, Err(RetainedSceneError::InvalidParentBranch(root)));
    assert_eq!(scene.version(), version);
    assert_eq!((scene.width, scene.height, scene.scale), (64, 64, 1.0));
    assert!(scene.nodes.contains_key(&b));
    assert!(!scene.nodes.contains_key(&invalid));
    assert_eq!(
        scene.nodes[&root]
            .content
            .order
            .iter()
            .map(|(&key, &id)| (key, id))
            .collect::<Vec<_>>(),
        order
    );
    let NodeKind::Scene { canvas, .. } = &scene.nodes[&a].kind else {
        unreachable!()
    };
    assert!(Arc::ptr_eq(canvas, &original));
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
fn move_before_changes_materialized_painter_order() {
    let root = RetainedNodeId::for_owner(1);
    let a = RetainedNodeId::for_owner(2);
    let b = RetainedNodeId::for_owner(3);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    transaction
        .insert_scene(
            RetainedParent::content(root),
            None,
            a,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            b,
            leaf(Color::BLACK),
            Affine::translate((0.0, 0.0)),
        );
    transaction.commit().unwrap();
    let mut transaction = scene.transaction();
    transaction.move_before(b, a).commit().unwrap();
    let materializer = PersistentSceneMaterializer::new(&scene);
    let frame = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert_eq!(
        frame.nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
        vec![b, a]
    );
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

    let descriptor =
        RetainedLayerDescriptor::ClipSdf(Sdf::Circle(crate::shared::sdf::circle::Circle {
            center: Point::new(f64::NAN, 8.0),
            radius: 4.0,
        }));
    assert_eq!(
        scene
            .transaction()
            .insert_layer(RetainedParent::content(root), None, layer, descriptor)
            .commit(),
        Err(RetainedSceneError::InvalidPosition)
    );
    assert_eq!(scene.version(), SceneVersion::INITIAL);
    assert!(!scene.nodes.contains_key(&layer));
}

#[test]
fn journal_merges_skipped_versions() {
    let root = RetainedNodeId::for_owner(1);
    let child = RetainedNodeId::for_owner(2);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    transaction.insert_scene(
        RetainedParent::content(root),
        None,
        child,
        leaf(Color::WHITE),
        Affine::translate((0.0, 0.0)),
    );
    transaction.commit().unwrap();
    let mut transaction = scene.transaction();
    transaction.set_transform(child, Affine::translate((4.0, 5.0)));
    transaction.commit().unwrap();
    let changes = scene.changes_since(SceneVersion::INITIAL).unwrap();
    assert!(changes.changed_nodes.contains(&child));
    assert!(changes.topology_changed);
}

#[test]
fn journal_gap_requires_one_full_resynchronization() {
    let root = RetainedNodeId::for_owner(1);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    for _ in 0..=JOURNAL_CAPACITY {
        scene
            .transaction()
            .invalidate_rect(Rect::new(0.0, 0.0, 1.0, 1.0))
            .commit()
            .unwrap();
    }

    assert!(scene.changes_since(SceneVersion::INITIAL).is_none());
    let recent = SceneVersion(scene.version().get() - 1);
    assert!(scene.changes_since(recent).is_some());
}

#[test]
fn journal_gap_detects_remove_and_reinsert_of_the_same_node_id() {
    let root = RetainedNodeId::for_owner(81_000);
    let child = RetainedNodeId::for_owner(81_001);
    let first = leaf(Color::WHITE);
    let replacement = leaf(Color::BLACK);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            first,
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);

    scene.transaction().remove_subtree(child).commit().unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            replacement.clone(),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    for _ in 0..255 {
        scene.transaction().invalidate_all().commit().unwrap();
    }

    assert!(materializer.update(&scene));
    assert!(Arc::ptr_eq(
        materializer.chunks[&child].source_canvas.as_ref().unwrap(),
        &replacement
    ));
    assert!(
        materializer
            .canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .full_scene_sync
    );
}

#[test]
fn long_content_delta_chain_compacts_into_state_pages() {
    let root = RetainedNodeId::for_owner(82_000);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let content = leaf(Color::WHITE);
    let mut transaction = scene.transaction();
    for index in 0..256 {
        transaction.insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(82_001 + index),
            content.clone(),
            Affine::translate((0.0, 0.0)),
        );
    }
    transaction.commit().unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let base_nodes = materializer
        .canvas
        .persistent_frame
        .as_ref()
        .unwrap()
        .nodes
        .clone();

    for index in 0..256 {
        scene
            .transaction()
            .replace_scene(RetainedNodeId::for_owner(82_001 + index), content.clone())
            .commit()
            .unwrap();
        assert!(materializer.update(&scene));
    }

    let frame = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(Arc::ptr_eq(&frame.nodes, &base_nodes));
    assert!(!frame.state_pages.is_empty());
    assert_eq!(frame.delta.as_ref().unwrap().depth, 1);
    assert!(frame.delta.as_ref().unwrap().previous.is_none());
    assert_eq!(
        frame.node_revision(RetainedNodeId::for_owner(82_001)),
        Some(NodeGeneration::new(1))
    );
}

#[test]
fn appended_root_layer_fragment_is_visible_in_cached_execution_plan() {
    let root = RetainedNodeId::for_owner(30);
    let base = RetainedNodeId::for_owner(31);
    let layer = RetainedNodeId::for_owner(32);
    let child = RetainedNodeId::for_owner(33);
    let mut scene = RetainedScene::new(64, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            leaf(Color::from_rgb8(20, 40, 220)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let base_batch = materializer.node_batches[&base];
    let base_frame = materializer.canvas.persistent_frame.clone().unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.5,
            },
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            child,
            leaf(Color::from_rgb8(220, 40, 20)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    assert!(materializer.update(&scene));
    assert_eq!(materializer.node_batches[&base], base_batch);
    let plan = materializer
        .canvas
        .compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    assert_eq!(
        plan.ops
            .iter()
            .filter(|op| matches!(op, crate::shared::execution::ExecOp::DrawBatch { .. }))
            .count(),
        2,
        "patched root plan: {:#?}",
        plan.ops
    );
    let inserted_frame = materializer.canvas.persistent_frame.clone().unwrap();
    assert!(
        Arc::ptr_eq(&base_frame.nodes, &inserted_frame.nodes),
        "root layer insertion must patch the immutable frame instead of collecting every node"
    );
    assert!(inserted_frame.node_state(layer).is_some());
    assert!(inserted_frame.node_state(child).is_some());
    assert_eq!(inserted_frame.delta.as_ref().unwrap().depth, 1);

    scene.transaction().remove_subtree(layer).commit().unwrap();
    assert!(materializer.update(&scene));
    let removed_frame = materializer.canvas.persistent_frame.clone().unwrap();
    assert!(Arc::ptr_eq(&base_frame.nodes, &removed_frame.nodes));
    assert!(removed_frame.node_state(layer).is_none());
    assert!(removed_frame.node_state(child).is_none());
    assert_eq!(removed_frame.delta.as_ref().unwrap().depth, 1);
}
