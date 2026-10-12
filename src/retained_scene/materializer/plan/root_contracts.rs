//! Root edits retain authoritative command order, revisions and fragment ownership.
use super::*;
use crate::{
    Radius, Region,
    shared::execution::{ExecOp, OffscreenLayer},
};
use peniko::{Color, kurbo::Rect};

fn filter(opacity: f32) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Filter {
        filter: filter::Filter::Opacity(opacity),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
    }
}

fn leaf() -> Rc<Canvas> {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO, Color::WHITE);
    Rc::new(canvas)
}

fn scene() -> RetainedScene {
    let id = RetainedNodeId::for_owner;
    let mut scene = RetainedScene::new(32, 32, 1.0, id(185_000)).unwrap();
    let mut tx = scene.transaction();
    for index in 0..3 {
        tx.insert_layer(
            RetainedParent::content(id(185_000)),
            None,
            id(185_001 + index * 2),
            filter(1.0),
        )
        .insert_scene(
            RetainedParent::content(id(185_001 + index * 2)),
            None,
            id(185_002 + index * 2),
            leaf(),
            Affine::IDENTITY,
        );
    }
    tx.commit().unwrap();
    scene
}

fn root_commands(state: &PersistentSceneMaterializer) -> Vec<RetainedNodeId> {
    state.canvas.command_lists[0]
        .commands
        .iter()
        .map(|command| match command {
            Command::Layer {
                retained: Some(key),
                ..
            }
            | Command::MaskLayer {
                retained: Some(key),
                ..
            } => key.id,
            _ => panic!("expected retained root layer command"),
        })
        .collect()
}

#[test]
fn root_reorder_and_parameter_update_preserve_the_new_filter() {
    let id = RetainedNodeId::for_owner;
    let mut scene = scene();
    let mut state = PersistentSceneMaterializer::new(&scene);
    scene
        .transaction()
        .update_layer(id(185_001), filter(0.25))
        .move_before(id(185_003), id(185_001))
        .commit()
        .unwrap();
    assert!(state.update(&scene, scene.changes_since(state.version())));
    let plan = state.canvas.compiled_plan.as_ref().unwrap();
    let opacity = plan.ops.iter().find_map(|op| match op {
        ExecOp::OffscreenLayer {
            retained_id: Some(key),
            layer:
                OffscreenLayer::Filter {
                    filter: filter::Filter::Opacity(value),
                    ..
                },
            ..
        } if key.node == id(185_001) => Some(*value),
        _ => None,
    });
    assert_eq!(
        opacity,
        Some(0.25),
        "reordering must not retain an older filter revision"
    );
}

#[test]
fn root_reorder_preserves_fragment_ranges_and_unrelated_draw_storage_on_removal() {
    let id = RetainedNodeId::for_owner;
    let mut scene = scene();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let old = state.canvas.compiled_plan.clone().unwrap();
    scene
        .transaction()
        .move_before(id(185_005), id(185_001))
        .commit()
        .unwrap();
    assert!(state.update(&scene, scene.changes_since(state.version())));
    for (index, node) in [id(185_005), id(185_001), id(185_003)]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            state
                .root_plan_fragments
                .get(&node)
                .map(|fragment| fragment.ops.clone()),
            Some(index..index + 1)
        );
    }
    let reordered = state.canvas.compiled_plan.clone().unwrap();
    scene
        .transaction()
        .remove_subtree(id(185_001))
        .commit()
        .unwrap();
    assert!(state.update(&scene, scene.changes_since(state.version())));
    let current = state.canvas.compiled_plan.as_ref().unwrap();
    let child_draws = |plan: &crate::shared::execution::ExecPlan, node| {
        plan.ops
            .iter()
            .find_map(|op| match op {
                ExecOp::OffscreenLayer {
                    retained_id: Some(key),
                    children,
                    ..
                } if key.node == node => match &children[0] {
                    ExecOp::DrawBatch { draws, .. } => Some(draws.clone()),
                    _ => None,
                },
                _ => None,
            })
            .unwrap()
    };
    for node in [id(185_005), id(185_003)] {
        assert!(
            Rc::ptr_eq(&child_draws(&reordered, node), &child_draws(current, node)),
            "unrelated root subtree was recompiled"
        );
    }
    assert_eq!(old.ops.len(), 3);
    assert_eq!(reordered.ops.len(), 3);
    assert_eq!(root_commands(&state), [id(185_005), id(185_003)]);
}

#[test]
fn inserting_a_root_layer_in_the_middle_keeps_authoritative_command_order() {
    let id = RetainedNodeId::for_owner;
    let mut scene = scene();
    let mut state = PersistentSceneMaterializer::new(&scene);
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(id(185_000)),
            Some(id(185_003)),
            id(185_010),
            filter(0.5),
        )
        .insert_scene(
            RetainedParent::content(id(185_010)),
            None,
            id(185_011),
            leaf(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(state.update(&scene, scene.changes_since(state.version())));
    assert_eq!(
        root_commands(&state),
        [id(185_001), id(185_010), id(185_003), id(185_005)]
    );
}

#[test]
fn promoting_an_offscreen_child_to_root_publishes_a_current_compiled_plan() {
    let id = RetainedNodeId::for_owner;
    let mut scene = scene();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(id(185_001)),
            None,
            id(185_010),
            filter(0.5),
        )
        .insert_scene(
            RetainedParent::content(id(185_010)),
            None,
            id(185_011),
            leaf(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    scene
        .transaction()
        .reparent(
            id(185_010),
            RetainedParent::content(id(185_000)),
            Some(id(185_003)),
        )
        .commit()
        .unwrap();
    assert!(state.update(&scene, scene.changes_since(state.version())));
    let plan = state
        .canvas
        .compiled_plan
        .as_ref()
        .expect("published materialization must own its current plan");
    assert_eq!(plan.ops.len(), 4);
    assert_eq!(
        root_commands(&state),
        [id(185_001), id(185_010), id(185_003), id(185_005)]
    );
}

#[test]
fn root_reorder_then_removal_matches_fresh_for_every_layer_kind_pair() {
    let id = RetainedNodeId::for_owner;
    let shape = |plan: &crate::shared::execution::ExecPlan| {
        plan.ops
            .iter()
            .map(std::mem::discriminant)
            .collect::<Vec<_>>()
    };
    for first in 0..8 {
        for second in 0..8 {
            let mut scene = RetainedScene::new(32, 32, 1.0, id(187_000)).unwrap();
            let mut tx = scene.transaction();
            for (node, kind) in [(187_001, first), (187_003, second)] {
                tx.insert_layer(
                    RetainedParent::content(id(187_000)),
                    None,
                    id(node),
                    super::tests::descriptor(kind),
                )
                .insert_scene(
                    RetainedParent::content(id(node)),
                    None,
                    id(node + 1),
                    leaf(),
                    Affine::IDENTITY,
                );
            }
            tx.commit().unwrap();
            let mut state = PersistentSceneMaterializer::new(&scene);
            let old = state.canvas.compiled_plan.clone().unwrap();
            let old_shape = shape(&old);
            scene
                .transaction()
                .move_before(id(187_003), id(187_001))
                .commit()
                .unwrap();
            assert!(state.update(&scene, scene.changes_since(state.version())));
            assert_eq!(root_commands(&state), [id(187_003), id(187_001)]);
            let fresh = PersistentSceneMaterializer::new(&scene);
            assert_eq!(
                shape(state.canvas.compiled_plan.as_ref().unwrap()),
                shape(fresh.canvas.compiled_plan.as_ref().unwrap()),
                "kinds={first},{second}"
            );
            assert_eq!(state.root_plan_fragments.len(), 2);
            scene
                .transaction()
                .remove_subtree(id(187_003))
                .commit()
                .unwrap();
            assert!(state.update(&scene, scene.changes_since(state.version())));
            assert_eq!(root_commands(&state), [id(187_001)]);
            let fresh = PersistentSceneMaterializer::new(&scene);
            assert_eq!(
                shape(state.canvas.compiled_plan.as_ref().unwrap()),
                shape(fresh.canvas.compiled_plan.as_ref().unwrap()),
                "removed kinds={first},{second}"
            );
            assert_eq!(shape(&old), old_shape);
        }
    }
}
