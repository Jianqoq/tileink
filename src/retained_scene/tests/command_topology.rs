//! Flat command synchronization follows node instances and current execution shape.
use super::*;
use crate::{Region, shared::execution::ExecOp};

fn scene(group: bool) -> (RetainedScene, RetainedNodeId) {
    let root = RetainedNodeId::for_owner(188_000);
    let target = RetainedNodeId::for_owner(188_001);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut tx = scene.transaction();
    if group {
        tx.insert_group(RetainedParent::content(root), None, target);
    } else {
        tx.insert_scene(
            RetainedParent::content(root),
            None,
            target,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        );
    }
    for index in 0..8 {
        tx.insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(188_010 + index),
            leaf(Color::BLACK),
            Affine::IDENTITY,
        );
    }
    tx.commit().unwrap();
    (scene, target)
}

fn filtered_leaf() -> Rc<Canvas> {
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::Opacity(0.25),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO),
    );
    canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, Color::WHITE);
    canvas.pop_layer();
    Rc::new(canvas)
}

fn offscreen_count(ops: &[ExecOp]) -> usize {
    ops.iter()
        .map(|op| match op {
            ExecOp::DrawBatch { .. } => 0,
            ExecOp::OffscreenLayer { children, .. } => 1 + offscreen_count(children),
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                1 + offscreen_count(content) + offscreen_count(mask)
            }
        })
        .sum()
}

#[test]
fn reinserting_a_scene_as_an_empty_group_retires_its_command() {
    let (mut scene, target) = scene(false);
    let mut state = PersistentSceneMaterializer::new(&scene);
    let root = scene.root();
    scene
        .transaction()
        .remove_subtree(target)
        .insert_group(RetainedParent::content(root), None, target)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    let fresh = PersistentSceneMaterializer::new(&scene);
    assert_eq!(
        root_retained_commands(&state),
        root_retained_commands(&fresh)
    );
}

#[test]
fn reinserting_an_empty_group_as_a_scene_preserves_the_flat_plan() {
    let (mut scene, target) = scene(true);
    let mut state = PersistentSceneMaterializer::new(&scene);
    let plan = std::rc::Rc::clone(state.canvas.compiled_plan.as_ref().unwrap());
    let root = scene.root();
    scene
        .transaction()
        .remove_subtree(target)
        .insert_scene(
            RetainedParent::content(root),
            None,
            target,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    let fresh = PersistentSceneMaterializer::new(&scene);
    assert_eq!(
        root_retained_commands(&state),
        root_retained_commands(&fresh)
    );
    assert!(Rc::ptr_eq(
        &plan,
        state.canvas.compiled_plan.as_ref().unwrap()
    ));
}

#[test]
fn inserting_a_scene_with_offscreen_content_compiles_its_operations() {
    let (mut scene, _) = scene(false);
    let mut state = PersistentSceneMaterializer::new(&scene);
    let root = scene.root();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(188_100),
            filtered_leaf(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    let fresh = PersistentSceneMaterializer::new(&scene);
    assert_eq!(
        offscreen_count(&state.canvas.compiled_plan.as_ref().unwrap().ops),
        offscreen_count(&fresh.canvas.compiled_plan.as_ref().unwrap().ops)
    );
    assert_eq!(
        offscreen_count(&state.canvas.compiled_plan.as_ref().unwrap().ops),
        1
    );
}

#[test]
fn reinserting_a_scene_with_offscreen_content_compiles_its_operations() {
    let (mut scene, target) = scene(false);
    let mut state = PersistentSceneMaterializer::new(&scene);
    let root = scene.root();
    scene
        .transaction()
        .remove_subtree(target)
        .insert_scene(
            RetainedParent::content(root),
            None,
            target,
            filtered_leaf(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    let fresh = PersistentSceneMaterializer::new(&scene);
    assert_eq!(
        offscreen_count(&state.canvas.compiled_plan.as_ref().unwrap().ops),
        offscreen_count(&fresh.canvas.compiled_plan.as_ref().unwrap().ops)
    );
    assert_eq!(
        offscreen_count(&state.canvas.compiled_plan.as_ref().unwrap().ops),
        1
    );
}

#[test]
fn appending_plain_content_after_offscreen_content_uses_a_new_run() {
    let (mut scene, target) = scene(false);
    let root = scene.root();
    scene
        .transaction()
        .replace_scene(target, filtered_leaf())
        .reparent(target, RetainedParent::content(root), None)
        .commit()
        .unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let added = RetainedNodeId::for_owner(188_101);
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            added,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    let batch = |id| state.canvas.painter.batch_ids[state.node_physical_draws(id).next().unwrap()];
    assert_ne!(batch(added), batch(RetainedNodeId::for_owner(188_010)));
}

#[test]
fn moving_plain_content_across_offscreen_content_changes_its_batch() {
    let (mut scene, target) = scene(false);
    scene
        .transaction()
        .replace_scene(target, filtered_leaf())
        .commit()
        .unwrap();
    let root = scene.root();
    let before = RetainedNodeId::for_owner(188_102);
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            Some(target),
            before,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    scene
        .transaction()
        .reparent(before, RetainedParent::content(root), None)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    let batch = |id| state.canvas.painter.batch_ids[state.node_physical_draws(id).next().unwrap()];
    assert_eq!(batch(before), batch(RetainedNodeId::for_owner(188_017)));
}

#[test]
fn moving_plain_content_inside_a_layer_synchronizes_its_command_order() {
    let root = RetainedNodeId::for_owner(188_200);
    let layer = RetainedNodeId::for_owner(188_201);
    let ids = [188_202, 188_203, 188_204].map(RetainedNodeId::for_owner);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut tx = scene.transaction();
    tx.insert_layer(
        RetainedParent::content(root),
        None,
        layer,
        RetainedLayerDescriptor::ClipPath {
            path: Rect::new(0.0, 0.0, 32.0, 32.0).to_path(0.1),
            transform: Affine::IDENTITY,
            rule: FillRule::NonZero,
            tolerance: 0.1,
        },
    );
    for id in ids {
        tx.insert_scene(
            RetainedParent::content(layer),
            None,
            id,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        );
    }
    tx.commit().unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    scene
        .transaction()
        .reparent(ids[0], RetainedParent::content(layer), None)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    let Command::Layer { children, .. } = state.canvas.command_lists[0].commands[0] else {
        unreachable!()
    };
    let commands = state.canvas.command_lists[children]
        .commands
        .iter()
        .map(|command| match command {
            Command::MaterializedRetainedScene { id, .. } => *id,
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(commands, [ids[1], ids[2], ids[0]]);
}

#[path = "command_topology/instances.rs"]
mod instances;
