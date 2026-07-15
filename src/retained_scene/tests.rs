use peniko::{Color, kurbo::Shape};

use super::*;
use crate::{Radius, shared::bounds::PixelBounds};

fn update_materializer(
    materializer: &mut PersistentSceneMaterializer,
    scene: &RetainedScene,
) -> bool {
    let changes = scene.changes_since(materializer.version());
    materializer.update(scene, changes)
}

fn leaf(color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, color);
    Rc::new(canvas)
}

fn multi_draw_leaf(draws: usize) -> Rc<Canvas> {
    let mut canvas = Canvas::new(256, 256, 1.0);
    for index in 0..draws {
        let x = (index % 16) as f64 * 12.0;
        let y = (index / 16) as f64 * 12.0;
        canvas.push_rect(
            Rect::new(x, y, x + 8.0, y + 8.0),
            Radius::ZERO,
            Color::WHITE,
        );
    }
    Rc::new(canvas)
}

fn empty_leaf() -> Rc<Canvas> {
    Rc::new(Canvas::new(16, 16, 1.0))
}

fn root_retained_commands(materializer: &PersistentSceneMaterializer) -> Vec<RetainedNodeId> {
    materializer.canvas.command_lists[0]
        .commands
        .iter()
        .map(|command| match command {
            Command::MaterializedRetainedScene { id, .. } => *id,
            Command::Layer {
                retained: Some(key),
                ..
            }
            | Command::MaskLayer {
                retained: Some(key),
                ..
            } => key.id,
            _ => unreachable!("persistent root only contains retained commands"),
        })
        .collect()
}

fn backdrop_leaf() -> Rc<Canvas> {
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_backdrop_layer(
        Filter::Opacity(0.5),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO),
    );
    canvas.pop_layer();
    Rc::new(canvas)
}

fn path_leaf() -> Rc<Canvas> {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_path(
        Rect::new(4.0, 6.0, 20.0, 18.0).to_path(0.1),
        Color::WHITE,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    Rc::new(canvas)
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
fn flat_topology_commands_patch_order_and_reuse_fragment_storage() {
    let root = RetainedNodeId::for_owner(70_005);
    let base = RetainedNodeId::for_owner(70_006);
    let anchor = RetainedNodeId::for_owner(70_007);
    let first = RetainedNodeId::for_owner(70_008);
    let second = RetainedNodeId::for_owner(70_009);
    let recycled = RetainedNodeId::for_owner(70_010);
    let content = leaf(Color::WHITE);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            content.clone(),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            anchor,
            content.clone(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let initial_command_lists = materializer.canvas.command_lists.len();

    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            Some(anchor),
            first,
            content.clone(),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            Some(anchor),
            second,
            content.clone(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_eq!(
        root_retained_commands(&materializer),
        vec![base, first, second, anchor]
    );

    scene
        .transaction()
        .remove_subtree(first)
        .insert_scene(
            RetainedParent::content(root),
            Some(second),
            recycled,
            content.clone(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_eq!(
        root_retained_commands(&materializer),
        vec![base, recycled, second, anchor]
    );
    assert_eq!(
        materializer.canvas.command_lists.len(),
        initial_command_lists + 2,
        "the replacement reuses the removed leaf's command fragment"
    );

    scene
        .transaction()
        .replace_scene(recycled, leaf(Color::BLACK))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
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
fn successful_rebalance_reports_every_sibling_with_a_new_order_key() {
    let root = RetainedNodeId::for_owner(20);
    let first = RetainedNodeId::for_owner(21);
    let tail = RetainedNodeId::for_owner(22);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            first,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            tail,
            leaf(Color::BLACK),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();

    let mut owner = 23;
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
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        owner += 1;
    }

    let previous_version = scene.version();
    let previous_siblings = scene.nodes[&root]
        .content
        .values()
        .copied()
        .collect::<Vec<_>>();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            Some(tail),
            RetainedNodeId::for_owner(owner),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();

    let changes = scene.changes_since(previous_version).unwrap();
    assert!(
        previous_siblings
            .iter()
            .all(|id| changes.changed_nodes.contains(id)),
        "rebalance changes every existing sibling order key"
    );
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
    assert!(update_materializer(&mut materializer, &scene));
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
    assert_eq!(canvas.logical_size(), (96, 32));
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
    let before = materializer.canvas.painter_keys.clone().unwrap();
    assert_eq!(
        before.iter().filter(|key| key.path[0] != u128::MAX).count(),
        3
    );

    scene.transaction().resize(96, 32, 1.0).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    // Surface remapping dirties live draw records but does not change their painter ownership.
    // Clearing those keys made the resize-only tile index omit every unchanged vector draw.
    assert_eq!(materializer.canvas.painter_keys.as_ref().unwrap(), &before);
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
fn same_scale_resize_and_removal_rebuild_spatial_index_from_live_nodes() {
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

    scene
        .transaction()
        .resize(96, 64, 2.0)
        .remove_subtree(removed)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    assert!(!materializer.chunks.contains_key(&removed));
    assert!(materializer.chunks.contains_key(&retained));
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
}

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

#[test]
fn materialized_dirty_range_buffers_rotate_between_frames() {
    let root = RetainedNodeId::for_owner(62_077);
    let child = RetainedNodeId::for_owner(62_078);
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
    let initial = materializer.canvas.buffer_changes.as_ref().unwrap();
    assert!(!initial.draws.is_empty());
    let initial_draw_ranges = initial.draws.as_ptr();

    scene
        .transaction()
        .replace_scene(child, leaf(Color::BLACK))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(
        !materializer
            .canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .draws
            .is_empty()
    );

    scene
        .transaction()
        .replace_scene(child, leaf(Color::WHITE))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let third = materializer.canvas.buffer_changes.as_ref().unwrap();

    // SceneArena and SceneBufferChanges exchange their allocations each sync. Equal-sized edits
    // therefore return to the first frame's range buffer after one intervening frame.
    assert_eq!(third.draws.as_ptr(), initial_draw_ranges);
}

#[test]
fn cached_node_draw_order_tracks_content_and_arena_relocation() {
    let root = RetainedNodeId::for_owner(62_080);
    let removed_a = RetainedNodeId::for_owner(62_081);
    let removed_b = RetainedNodeId::for_owner(62_082);
    let retained = RetainedNodeId::for_owner(62_083);
    let inserted = RetainedNodeId::for_owner(62_084);
    let mut scene = RetainedScene::new(256, 256, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            removed_a,
            multi_draw_leaf(4),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            removed_b,
            multi_draw_leaf(4),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            retained,
            multi_draw_leaf(4),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let before = materializer
        .node_physical_draws(retained)
        .collect::<Vec<_>>();
    let compactions = materializer.arenas.draws.compactions();

    scene
        .transaction()
        .remove_subtree(removed_a)
        .remove_subtree(removed_b)
        .insert_scene(
            RetainedParent::content(root),
            None,
            inserted,
            multi_draw_leaf(20),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(materializer.arenas.draws.compactions() > compactions);
    let after = materializer
        .node_physical_draws(retained)
        .collect::<Vec<_>>();
    assert_ne!(after, before);
    let chunk = &materializer.chunks[&retained];
    let base = materializer.arenas.draws.range(chunk.draws).start;
    let expected = chunk
        .canvas
        .compile(crate::shared::execution::ROOT_COMMAND_LIST_ID)
        .draw_order
        .iter()
        .map(|&draw| base + draw as usize)
        .collect::<Vec<_>>();
    assert_eq!(after, expected);

    scene
        .transaction()
        .replace_scene(retained, multi_draw_leaf(6))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let after_replacement = materializer
        .node_physical_draws(retained)
        .collect::<Vec<_>>();
    assert_eq!(after_replacement.len(), 6);
    let chunk = &materializer.chunks[&retained];
    let base = materializer.arenas.draws.range(chunk.draws).start;
    let expected = chunk
        .canvas
        .compile(crate::shared::execution::ROOT_COMMAND_LIST_ID)
        .draw_order
        .iter()
        .map(|&draw| base + draw as usize)
        .collect::<Vec<_>>();
    assert_eq!(after_replacement, expected);
}

#[test]
fn resize_with_layer_updates_defers_full_frame_and_spatial_rebuild() {
    let root = RetainedNodeId::for_owner(62_100);
    let clip = RetainedNodeId::for_owner(62_101);
    let child = RetainedNodeId::for_owner(62_102);
    let descriptor = |width| RetainedLayerDescriptor::ClipSdf {
        sdf: Sdf::Rect(crate::SdfRect {
            start: Point::ZERO,
            end: Point::new(width, 64.0),
            radius: Radius::ZERO,
        }),
        transform: Affine::IDENTITY,
    };
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(RetainedParent::content(root), None, clip, descriptor(64.0))
        .insert_scene(
            RetainedParent::content(clip),
            None,
            child,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let base_nodes = materializer
        .canvas
        .persistent_frame
        .as_ref()
        .unwrap()
        .nodes
        .clone();

    scene
        .transaction()
        .resize(96, 64, 1.0)
        .update_layer(clip, descriptor(96.0))
        .set_transform(child, Affine::translate((8.0, 0.0)))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let resized = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(Rc::ptr_eq(&resized.nodes, &base_nodes));
    assert!(resized.invalidate_all);
    assert_eq!(resized.logical_size, (96, 64));
    assert_eq!(
        resized.node_state(clip).unwrap().revision,
        NodeGeneration::new(scene.nodes[&clip].generation),
        "resize frames must publish current revisions for retained surface cache keys"
    );
    assert!(materializer.surface_metadata_stale);

    // The first later incremental edit rebuilds the deferred baseline before applying its delta,
    // so damage and spatial queries observe the exact post-resize bounds.
    scene
        .transaction()
        .set_transform(child, Affine::translate((24.0, 0.0)))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let updated = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(!Rc::ptr_eq(&updated.nodes, &base_nodes));
    assert_eq!(
        updated.node_state(child).unwrap().bounds,
        Bounds::new(24, 0, 40, 16)
    );
    assert!(!materializer.surface_metadata_stale);
}

#[test]
fn structural_edit_after_resize_rebuilds_deferred_surface_metadata() {
    let root = RetainedNodeId::for_owner(62_110);
    let existing = RetainedNodeId::for_owner(62_111);
    let inserted = RetainedNodeId::for_owner(62_112);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            existing,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);

    scene.transaction().resize(96, 64, 1.0).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(materializer.surface_metadata_stale);

    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            inserted,
            leaf(Color::BLACK),
            Affine::translate((48.0, 0.0)),
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));

    let frame = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(frame.node_state(existing).is_some());
    assert!(frame.node_state(inserted).is_some());
    assert_eq!(frame.logical_size, (96, 64));
    assert_eq!(
        materializer.spatial_tiles_size,
        (
            materializer.canvas.width_in_tiles(),
            materializer.canvas.height_in_tiles(),
        )
    );
    assert!(!materializer.surface_metadata_stale);
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
    assert!(update_materializer(&mut materializer, &scene));
    let chunk = &materializer.chunks[&child];
    assert_eq!(materializer.arenas.draws.range(chunk.draws).len(), 1);
    assert!(!materializer.arenas.sdfs.range(chunk.sdfs).is_empty());

    scene
        .transaction()
        .replace_scene(child, empty_leaf())
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
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
    assert!(Rc::ptr_eq(canvas, &original));
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

    assert!(update_materializer(&mut materializer, &scene));
    assert!(Rc::ptr_eq(
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
        assert!(update_materializer(&mut materializer, &scene));
    }

    let frame = materializer.canvas.persistent_frame.as_ref().unwrap();
    assert!(Rc::ptr_eq(&frame.nodes, &base_nodes));
    assert!(!frame.state_pages.is_empty());
    assert_eq!(frame.delta.as_ref().unwrap().depth, 1);
    assert!(frame.delta.as_ref().unwrap().previous.is_none());
    assert_eq!(
        frame.node_revision(RetainedNodeId::for_owner(82_001)),
        Some(NodeGeneration::new(1))
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

    assert!(update_materializer(&mut materializer, &scene));
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
        Rc::ptr_eq(&base_frame.nodes, &inserted_frame.nodes),
        "root layer insertion must patch the immutable frame instead of collecting every node"
    );
    assert!(inserted_frame.node_state(layer).is_some());
    assert!(inserted_frame.node_state(child).is_some());
    assert_eq!(inserted_frame.delta.as_ref().unwrap().depth, 1);

    scene.transaction().remove_subtree(layer).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    let removed_frame = materializer.canvas.persistent_frame.clone().unwrap();
    assert!(Rc::ptr_eq(&base_frame.nodes, &removed_frame.nodes));
    assert!(removed_frame.node_state(layer).is_none());
    assert!(removed_frame.node_state(child).is_none());
    assert_eq!(removed_frame.delta.as_ref().unwrap().depth, 1);
}
