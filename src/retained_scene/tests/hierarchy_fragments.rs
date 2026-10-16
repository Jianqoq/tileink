//! A multi-fragment edit must publish the same scopes as compiling its final command tree.
use super::*;
use crate::shared::execution::{ExecOp, ExecPlan};

fn id(value: u64) -> RetainedNodeId {
    RetainedNodeId::for_owner(230_000 + value)
}

fn filter() -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Filter {
        filter: Filter::Opacity(0.5),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
    }
}

fn scene(mask: bool) -> RetainedScene {
    let mut scene = RetainedScene::new(32, 32, 1.0, id(0)).unwrap();
    let mut tx = scene.transaction();
    tx.insert_layer(RetainedParent::content(id(0)), None, id(12), filter());
    for (clip, parent, inner, child, x) in [(10, 1, 3, 4, 0.0), (11, 5, 6, 7, 16.0)] {
        tx.insert_layer(
            RetainedParent::content(id(12)),
            None,
            id(clip),
            RetainedLayerDescriptor::ClipSdf {
                sdf: crate::Sdf::Rect(crate::SdfRect {
                    start: (x, 0.0).into(),
                    end: (x + 16.0, 32.0).into(),
                    radius: Radius::ZERO,
                }),
                transform: Affine::IDENTITY,
            },
        );
        let owner = if mask { id(1) } else { id(parent) };
        if !mask || parent == 1 {
            tx.insert_layer(
                RetainedParent::content(id(clip)),
                None,
                owner,
                if mask {
                    RetainedLayerDescriptor::Mask(crate::Mask {
                        region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
                        kind: crate::MaskKind::Alpha,
                    })
                } else {
                    filter()
                },
            );
        }
        tx.insert_layer(
            if mask && parent == 5 {
                RetainedParent::mask(owner)
            } else {
                RetainedParent::content(owner)
            },
            None,
            id(inner),
            filter(),
        )
        .insert_scene(
            RetainedParent::content(id(inner)),
            None,
            id(child),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        );
    }
    tx.commit().unwrap();
    scene
}

fn shape(plan: &ExecPlan) -> Vec<String> {
    fn visit(ops: &[ExecOp], plan: &ExecPlan, out: &mut Vec<String>) {
        for op in ops {
            match op {
                ExecOp::DrawBatch {
                    draws, layer_stack, ..
                } => out.push(format!(
                    "draw {draws:?} {:?}",
                    &plan.layer_stack_data[layer_stack.clone()]
                )),
                ExecOp::OffscreenLayer {
                    retained_id,
                    layer,
                    outer_stack,
                    children,
                } => {
                    out.push(format!(
                        "layer {retained_id:?} {layer:?} {:?}",
                        &plan.layer_stack_data[outer_stack.clone()]
                    ));
                    visit(children, plan, out);
                    out.push("end".into());
                }
                ExecOp::OffscreenMaskLayer {
                    retained_id,
                    layer,
                    outer_stack,
                    content,
                    mask,
                } => {
                    out.push(format!(
                        "mask {retained_id:?} {layer:?} {:?}",
                        &plan.layer_stack_data[outer_stack.clone()]
                    ));
                    visit(content, plan, out);
                    out.push("mask branch".into());
                    visit(mask, plan, out);
                    out.push("end".into());
                }
            }
        }
    }
    let mut out = Vec::new();
    visit(&plan.ops, plan, &mut out);
    out
}

fn assert_current_plan(state: &PersistentSceneMaterializer, scene: &RetainedScene) {
    let cached = state.canvas.compiled_plan.as_ref().unwrap();
    let mut commands = (*state.canvas).clone();
    commands.compiled_plan = None;
    let compiled = commands.compile(0);
    assert_eq!(shape(cached), shape(&compiled));
    assert_eq!(cached.draw_order, compiled.draw_order);
    for (&node, data) in &scene.nodes {
        if !matches!(data.kind, NodeKind::Scene { .. }) {
            continue;
        }
        for draw in state.node_physical_draws(node) {
            assert_ne!(cached.draw_batch_ids[draw], u32::MAX);
            assert_eq!(
                cached.draw_batch_ids[draw],
                state.canvas.painter.batch_ids[draw]
            );
            assert_eq!(state.node_batches[&node], cached.draw_batch_ids[draw]);
        }
    }
}

#[test]
fn swapping_offscreen_subtrees_updates_both_ancestor_clip_scopes() {
    let mut scene = scene(false);
    let mut state = PersistentSceneMaterializer::new(&scene);
    for iteration in 0..24 {
        let (a, b) = if iteration % 2 == 0 { (5, 1) } else { (1, 5) };
        scene
            .transaction()
            .reparent(id(3), RetainedParent::content(id(a)), None)
            .reparent(id(6), RetainedParent::content(id(b)), None)
            .commit()
            .unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_current_plan(&state, &scene);
    }
}

#[test]
fn swapping_mask_branches_replaces_each_draw_membership() {
    let mut scene = scene(true);
    let mut state = PersistentSceneMaterializer::new(&scene);
    for iteration in 0..24 {
        let (a, b) = if iteration % 2 == 0 {
            (RetainedParent::mask(id(1)), RetainedParent::content(id(1)))
        } else {
            (RetainedParent::content(id(1)), RetainedParent::mask(id(1)))
        };
        scene
            .transaction()
            .reparent(id(3), a, None)
            .reparent(id(6), b, None)
            .commit()
            .unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_current_plan(&state, &scene);
    }
}

#[test]
fn moving_an_ancestor_and_child_rebuilds_overlapping_fragments_from_the_final_tree() {
    let mut scene = scene(false);
    let mut state = PersistentSceneMaterializer::new(&scene);
    for iteration in 0..24 {
        let (parent, child_parent) = if iteration % 2 == 0 { (5, 5) } else { (10, 1) };
        scene
            .transaction()
            .reparent(id(1), RetainedParent::content(id(parent)), None)
            .reparent(id(3), RetainedParent::content(id(child_parent)), None)
            .commit()
            .unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_current_plan(&state, &scene);
    }
}
