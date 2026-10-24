use super::*;
use crate::{Mask, MaskKind, Radius, Region, Sdf, SdfRect};
use peniko::{
    Color, Compose, Mix,
    kurbo::{Affine, Point, Rect, Shape},
};

pub(super) fn descriptor(kind: usize) -> RetainedLayerDescriptor {
    let rect = Rect::new(0.0, 0.0, 32.0, 32.0);
    let path = rect.to_path(0.1);
    let sample_region = Region::rect(rect, Radius::ZERO);
    match kind {
        0 => RetainedLayerDescriptor::ClipPath {
            path,
            transform: Affine::IDENTITY,
            rule: FillRule::NonZero,
            tolerance: 0.1,
        },
        1 => RetainedLayerDescriptor::ClipSdf {
            sdf: Sdf::Rect(SdfRect {
                start: Point::ZERO,
                end: Point::new(32.0, 32.0),
                radius: Radius::ZERO,
            }),
            transform: Affine::IDENTITY,
        },
        2 => RetainedLayerDescriptor::Opacity {
            path,
            transform: Affine::IDENTITY,
            tolerance: 0.1,
            opacity: 0.5,
        },
        3 => RetainedLayerDescriptor::Blend {
            path,
            transform: Affine::IDENTITY,
            tolerance: 0.1,
            mix: Mix::Screen,
            compose: Compose::SrcOver,
        },
        4 => RetainedLayerDescriptor::Isolate {
            path,
            transform: Affine::IDENTITY,
            tolerance: 0.1,
        },
        5 => RetainedLayerDescriptor::Filter {
            filter: filter::Filter::Opacity(1.0),
            sample_region,
        },
        6 => RetainedLayerDescriptor::Backdrop {
            filter: filter::Filter::Opacity(1.0),
            sample_region,
        },
        7 => RetainedLayerDescriptor::Mask(Mask {
            region: sample_region,
            kind: MaskKind::Alpha,
        }),
        _ => unreachable!(),
    }
}

fn leaf(empty: bool) -> Rc<Canvas> {
    let mut canvas = Canvas::new(32, 32, 1.0);
    if !empty {
        canvas.push_rect(Rect::new(2.0, 2.0, 30.0, 30.0), Radius::ZERO, Color::WHITE);
    }
    Rc::new(canvas)
}

fn assert_fragment_partition(state: &PersistentSceneMaterializer, scene: &RetainedScene) {
    let plan = state.canvas.compiled_plan.as_ref().unwrap();
    let mut covered = vec![false; plan.ops.len()];
    for (&id, fragment) in &state.root_plan_fragments {
        assert!(!fragment.ops.is_empty());
        assert!(fragment.ops.end <= plan.ops.len());
        let mut nodes = Vec::new();
        scene.collect_subtree(id, &mut nodes);
        assert_eq!(fragment.nodes, nodes.into_iter().collect());
        for index in fragment.ops.clone() {
            assert!(!covered[index], "two root fragments overlap");
            covered[index] = true;
        }
    }
    assert!(
        covered.iter().all(|&covered| covered),
        "a root layer lost its execution range"
    );
    assert_eq!(
        state.canvas.painter.batch_ids.len(),
        state.arenas.draws.values().len()
    );
}

#[test]
fn root_fragment_ranges_follow_all_layer_kind_transitions_and_empty_children() {
    let id = RetainedNodeId::for_owner;
    for empty in [false, true] {
        for before in 0..8 {
            for after in 0..8 {
                let mut scene = RetainedScene::new(32, 32, 1.0, id(181_000)).unwrap();
                scene
                    .transaction()
                    .insert_layer(
                        RetainedParent::content(id(181_000)),
                        None,
                        id(181_001),
                        descriptor(5),
                    )
                    .insert_layer(
                        RetainedParent::content(id(181_000)),
                        None,
                        id(181_002),
                        descriptor(before),
                    )
                    .insert_layer(
                        RetainedParent::content(id(181_002)),
                        None,
                        id(181_003),
                        descriptor(5),
                    )
                    .insert_scene(
                        RetainedParent::content(id(181_003)),
                        None,
                        id(181_004),
                        leaf(empty),
                        Affine::IDENTITY,
                    )
                    .insert_layer(
                        RetainedParent::content(id(181_000)),
                        None,
                        id(181_005),
                        descriptor(5),
                    )
                    .insert_scene(
                        RetainedParent::content(id(181_005)),
                        None,
                        id(181_006),
                        leaf(false),
                        Affine::IDENTITY,
                    )
                    .commit()
                    .unwrap();
                let mut state = PersistentSceneMaterializer::new(&scene);
                assert_fragment_partition(&state, &scene);
                let previous = state.canvas.compiled_plan.clone().unwrap();
                let previous_len = previous.ops.len();
                scene
                    .transaction()
                    .update_layer(id(181_002), descriptor(after))
                    .commit()
                    .unwrap();
                state.update(&scene, scene.changes_since(state.version()));
                assert_eq!(previous.ops.len(), previous_len);
                assert_fragment_partition(&state, &scene);
                // A non-tail removal must use the new range and preserve both neighbours.
                scene
                    .transaction()
                    .remove_subtree(id(181_002))
                    .commit()
                    .unwrap();
                assert!(state.update(&scene, scene.changes_since(state.version())));
                assert_fragment_partition(&state, &scene);
                let fresh = PersistentSceneMaterializer::new(&scene);
                assert_eq!(
                    state.canvas.compiled_plan.as_ref().unwrap().ops.len(),
                    fresh.canvas.compiled_plan.as_ref().unwrap().ops.len(),
                    "empty={empty}, {before}->{after}"
                );
                assert_eq!(state.root_plan_fragments.len(), 2);
            }
        }
    }
}

#[test]
fn a_large_inserted_run_keeps_the_existing_plan_and_batch() {
    let id = RetainedNodeId::for_owner;
    let root = id(182_000);
    let layer = id(182_001);
    let anchor = id(182_002);
    let mut scene = RetainedScene::new(32, 32, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(RetainedParent::content(root), None, layer, descriptor(5))
        .insert_scene(
            RetainedParent::content(layer),
            None,
            anchor,
            leaf(false),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let plan = state.canvas.compiled_plan.clone().unwrap();
    let nodes = std::rc::Rc::clone(&state.canvas.persistent_frame.as_ref().unwrap().nodes);
    let batch = state.node_batches[&anchor];
    let mut tx = scene.transaction();
    for index in 0..1024 {
        tx.insert_scene(
            RetainedParent::content(layer),
            None,
            id(183_000 + index),
            leaf(false),
            Affine::IDENTITY,
        );
    }
    tx.commit().unwrap();
    assert!(state.update(&scene, scene.changes_since(state.version())));
    assert!(Rc::ptr_eq(
        &plan,
        state.canvas.compiled_plan.as_ref().unwrap()
    ));
    assert!(Rc::ptr_eq(
        &nodes,
        &state.canvas.persistent_frame.as_ref().unwrap().nodes
    ));
    assert_eq!(
        state
            .canvas
            .buffer_changes
            .as_ref()
            .unwrap()
            .plan_fragments_rebuilt,
        0
    );
    for index in 0..1024 {
        let child = id(183_000 + index);
        assert_eq!(state.node_batches[&child], batch);
        for draw in state.node_physical_draws(child) {
            assert_eq!(state.canvas.painter.batch_ids[draw], batch);
        }
    }
}

#[test]
fn empty_mask_branches_keep_independent_batches_on_first_insertion() {
    let id = RetainedNodeId::for_owner;
    for branch in [RetainedChildBranch::Content, RetainedChildBranch::Mask] {
        let mut scene = RetainedScene::new(32, 32, 1.0, id(184_000)).unwrap();
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(id(184_000)),
                None,
                id(184_001),
                descriptor(7),
            )
            .commit()
            .unwrap();
        let mut state = PersistentSceneMaterializer::new(&scene);
        let content = state.empty_branch_batches[&RetainedParent::content(id(184_001))];
        let mask = state.empty_branch_batches[&RetainedParent::mask(id(184_001))];
        assert_ne!(content, mask);
        let plan = state.canvas.compiled_plan.clone().unwrap();
        scene
            .transaction()
            .insert_scene(
                RetainedParent {
                    node: id(184_001),
                    branch,
                },
                None,
                id(184_002),
                leaf(false),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        assert!(state.update(&scene, scene.changes_since(state.version())));
        assert!(Rc::ptr_eq(
            &plan,
            state.canvas.compiled_plan.as_ref().unwrap()
        ));
        assert_eq!(
            state.node_batches[&id(184_002)],
            if branch == RetainedChildBranch::Content {
                content
            } else {
                mask
            }
        );
    }
}
