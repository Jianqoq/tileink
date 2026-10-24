//! A local offscreen edit must cover every changed and retired node in its transaction.
use super::*;
use crate::shared::execution::{CommandListId, ExecOp};

fn id(value: u64) -> RetainedNodeId {
    RetainedNodeId::for_owner(190_000 + value)
}

fn filter() -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Filter {
        filter: Filter::Opacity(0.5),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
    }
}

fn scene() -> RetainedScene {
    let mut scene = RetainedScene::new(32, 32, 1.0, id(0)).unwrap();
    let mut tx = scene.transaction();
    for index in 10..18 {
        tx.insert_scene(
            RetainedParent::content(id(0)),
            None,
            id(index),
            leaf(Color::BLACK),
            Affine::IDENTITY,
        );
    }
    tx.insert_layer(RetainedParent::content(id(0)), None, id(1), filter())
        .insert_scene(
            RetainedParent::content(id(1)),
            None,
            id(2),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_layer(RetainedParent::content(id(1)), None, id(3), filter())
        .insert_scene(
            RetainedParent::content(id(3)),
            None,
            id(4),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(id(0)),
            None,
            id(5),
            leaf(Color::WHITE),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    scene
}

fn command_order(canvas: &Canvas, list: CommandListId) -> Vec<RetainedNodeId> {
    let mut ids = Vec::new();
    for command in &canvas.command_lists[list].commands {
        match command {
            Command::MaterializedRetainedScene { id, .. } => ids.push(*id),
            Command::Layer {
                retained: Some(key),
                children,
                ..
            } => {
                ids.push(key.id);
                ids.extend(command_order(canvas, *children));
            }
            Command::MaskLayer {
                retained: Some(key),
                content,
                mask,
                ..
            } => {
                ids.push(key.id);
                ids.extend(command_order(canvas, *content));
                ids.extend(command_order(canvas, *mask));
            }
            _ => panic!("retained container owns retained commands"),
        }
    }
    ids
}

fn has_draw(ops: &[ExecOp], draw: usize) -> bool {
    ops.iter().any(|op| match op {
        ExecOp::DrawBatch { draws, .. } => draws.contains(&draw),
        ExecOp::OffscreenLayer { children, .. } => has_draw(children, draw),
        ExecOp::OffscreenMaskLayer { content, mask, .. } => {
            has_draw(content, draw) || has_draw(mask, draw)
        }
    })
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

fn removal(edit: impl FnOnce(&mut RetainedSceneTransaction<'_>)) {
    let mut scene = scene();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let mut tx = scene.transaction();
    tx.remove_subtree(id(3));
    edit(&mut tx);
    tx.commit().unwrap();
    assert!(update_materializer(&mut state, &scene));
    let fresh = PersistentSceneMaterializer::new(&scene);
    assert_eq!(
        command_order(&state.canvas, 0),
        command_order(&fresh.canvas, 0)
    );
    let plan = state.canvas.compiled_plan.as_ref().unwrap();
    assert_eq!(
        offscreen_count(&plan.ops),
        offscreen_count(&fresh.canvas.compiled_plan.as_ref().unwrap().ops)
    );
    for (&node, data) in &scene.nodes {
        if matches!(data.kind, NodeKind::Scene { .. }) {
            for draw in state.node_physical_draws(node) {
                assert!(
                    has_draw(&plan.ops, draw),
                    "live node {node:?} absent from plan"
                );
            }
        }
    }
}

#[test]
fn nested_removal_and_root_scene_insertion_publish_both_changes() {
    removal(|tx| {
        tx.insert_scene(
            RetainedParent::content(id(0)),
            None,
            id(6),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        );
    });
}

#[test]
fn nested_removal_and_root_layer_insertion_publish_both_changes() {
    removal(|tx| {
        tx.insert_layer(RetainedParent::content(id(0)), None, id(6), filter())
            .insert_scene(
                RetainedParent::content(id(6)),
                None,
                id(7),
                leaf(Color::WHITE),
                Affine::IDENTITY,
            );
    });
}

#[test]
fn nested_removal_and_unrelated_scene_removal_retire_both_commands() {
    removal(|tx| {
        tx.remove_subtree(id(5));
    });
}

#[test]
fn nested_removal_and_reparenting_out_publish_the_new_command_scope() {
    removal(|tx| {
        tx.reparent(id(2), RetainedParent::content(id(0)), None);
    });
}

#[test]
fn nested_removal_and_reparenting_in_retire_the_previous_outer_command() {
    removal(|tx| {
        tx.reparent(id(5), RetainedParent::content(id(1)), None);
    });
}

#[test]
fn nested_removal_and_inside_insertion_publish_the_new_subtree() {
    removal(|tx| {
        tx.insert_scene(
            RetainedParent::content(id(1)),
            None,
            id(6),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        );
    });
}

#[test]
fn nested_removal_and_root_content_structure_change_publish_both_changes() {
    removal(|tx| {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_filter_layer(
            Filter::Opacity(0.25),
            Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO),
        );
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, Color::WHITE);
        canvas.pop_layer();
        tx.replace_scene(id(5), Rc::new(canvas));
    });
}

#[test]
fn nested_removal_and_ancestor_move_publish_the_new_painter_order() {
    removal(|tx| {
        tx.reparent(id(1), RetainedParent::content(id(0)), None);
    });
}

#[test]
fn nested_removal_and_ancestor_fusion_compile_the_current_structure() {
    removal(|tx| {
        tx.update_layer(
            id(1),
            RetainedLayerDescriptor::ClipPath {
                path: Rect::new(0.0, 0.0, 32.0, 32.0).to_path(0.1),
                transform: Affine::IDENTITY,
                rule: FillRule::NonZero,
                tolerance: 0.1,
            },
        );
    });
}

#[test]
fn local_nested_removal_and_insertion_keep_unrelated_draw_storage() {
    let mut scene = scene();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let old = state.canvas.compiled_plan.clone().unwrap();
    scene
        .transaction()
        .remove_subtree(id(3))
        .insert_scene(
            RetainedParent::content(id(1)),
            None,
            id(6),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    let current = state.canvas.compiled_plan.as_ref().unwrap();
    let ExecOp::DrawBatch {
        draws: old_draws, ..
    } = &old.ops[0]
    else {
        panic!("root background batch")
    };
    let ExecOp::DrawBatch {
        draws: current_draws,
        ..
    } = &current.ops[0]
    else {
        panic!("root background batch")
    };
    assert!(
        Rc::ptr_eq(old_draws, current_draws),
        "a local fragment edit must retain unrelated storage"
    );
}
