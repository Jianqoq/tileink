use super::*;
use crate::shared::execution::{ExecOp, OffscreenLayer};
use peniko::{BlendMode, Compose, Mix};

const GROUP: RetainedNodeId = RetainedNodeId::for_owner(71_001);
const FILTER: RetainedNodeId = RetainedNodeId::for_owner(71_002);

fn opacity(value: f32) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Opacity {
        path: Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity: value,
    }
}

fn blend(mix: Mix) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Blend {
        path: Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        mix,
        compose: Compose::SrcOver,
    }
}

fn filter(value: f32) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Filter {
        filter: Filter::Opacity(value),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO),
    }
}

fn scene(group: RetainedLayerDescriptor) -> RetainedScene {
    let root = RetainedNodeId::for_owner(71_000);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(RetainedParent::content(root), None, GROUP, group)
        .insert_layer(RetainedParent::content(GROUP), None, FILTER, filter(1.0))
        .insert_scene(
            RetainedParent::content(FILTER),
            None,
            RetainedNodeId::for_owner(71_003),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    scene
}

fn root_layer(materializer: &PersistentSceneMaterializer) -> &OffscreenLayer {
    let plan = materializer.canvas.compiled_plan.as_ref().unwrap();
    let ExecOp::OffscreenLayer {
        retained_id: Some(id),
        layer,
        ..
    } = &plan.ops[0]
    else {
        panic!("group with offscreen child must own an offscreen operation");
    };
    assert_eq!(id.node, GROUP);
    layer
}

#[test]
fn offscreen_opacity_updates_its_compiled_descriptor_without_rebuilding_batches() {
    let mut scene = scene(opacity(0.25));
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let order = std::rc::Rc::clone(
        &materializer
            .canvas
            .compiled_plan
            .as_ref()
            .unwrap()
            .draw_order,
    );
    for value in [0.75, 0.0, 1.0] {
        scene
            .transaction()
            .update_layer(GROUP, opacity(value))
            .commit()
            .unwrap();
        assert!(update_materializer(&mut materializer, &scene));
        assert!(
            matches!(root_layer(&materializer), OffscreenLayer::Opacity { opacity, .. } if *opacity == value)
        );
        assert!(Rc::ptr_eq(
            &order,
            &materializer
                .canvas
                .compiled_plan
                .as_ref()
                .unwrap()
                .draw_order
        ));
    }
}

#[test]
fn offscreen_blend_updates_its_compiled_descriptor_without_rebuilding_batches() {
    let mut scene = scene(blend(Mix::Multiply));
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let order = std::rc::Rc::clone(
        &materializer
            .canvas
            .compiled_plan
            .as_ref()
            .unwrap()
            .draw_order,
    );
    for mix in [Mix::Screen, Mix::Normal] {
        scene
            .transaction()
            .update_layer(GROUP, blend(mix))
            .commit()
            .unwrap();
        assert!(update_materializer(&mut materializer, &scene));
        assert!(
            matches!(root_layer(&materializer), OffscreenLayer::Blend { mode, .. } if *mode == BlendMode::new(mix, Compose::SrcOver))
        );
        assert!(Rc::ptr_eq(
            &order,
            &materializer
                .canvas
                .compiled_plan
                .as_ref()
                .unwrap()
                .draw_order
        ));
    }
}

#[test]
fn filter_updates_do_not_remove_an_unrelated_fused_layer_index() {
    let mut scene = scene(RetainedLayerDescriptor::ClipSdf {
        sdf: crate::Sdf::Rect(crate::SdfRect {
            start: Point::ZERO,
            end: Point::new(64.0, 64.0),
            radius: Radius::ZERO,
        }),
        transform: Affine::IDENTITY,
    });
    // An empty filter has no draw record; the clip is the only physical draw.
    // Its index must survive updates of the filter's unrelated descriptor.
    scene
        .transaction()
        .remove_subtree(RetainedNodeId::for_owner(71_003))
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let locations = materializer
        .canvas
        .compiled_plan
        .as_ref()
        .unwrap()
        .layer_stack_locations
        .clone();
    assert!(!locations.is_empty());
    for value in [0.5, 1.0] {
        scene
            .transaction()
            .update_layer(FILTER, filter(value))
            .commit()
            .unwrap();
        assert!(update_materializer(&mut materializer, &scene));
        assert_eq!(
            materializer
                .canvas
                .compiled_plan
                .as_ref()
                .unwrap()
                .layer_stack_locations,
            locations
        );
    }
}

#[test]
fn effect_parameter_updates_do_not_upload_unmodified_clip_stacks() {
    for backdrop in [false, true] {
        for empty in [false, true] {
            let effect = |value| {
                let sample_region = Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO);
                if backdrop {
                    RetainedLayerDescriptor::Backdrop {
                        filter: Filter::Opacity(value),
                        sample_region,
                    }
                } else {
                    RetainedLayerDescriptor::Filter {
                        filter: Filter::Opacity(value),
                        sample_region,
                    }
                }
            };
            let mut scene = scene(RetainedLayerDescriptor::ClipSdf {
                sdf: crate::Sdf::Rect(crate::SdfRect {
                    start: Point::ZERO,
                    end: Point::new(64.0, 64.0),
                    radius: Radius::ZERO,
                }),
                transform: Affine::IDENTITY,
            });
            scene
                .transaction()
                .update_layer(FILTER, effect(1.0))
                .commit()
                .unwrap();
            if empty {
                scene
                    .transaction()
                    .remove_subtree(RetainedNodeId::for_owner(71_003))
                    .commit()
                    .unwrap();
            }
            let mut materializer = PersistentSceneMaterializer::new(&scene);
            let stack = materializer
                .canvas
                .compiled_plan
                .as_ref()
                .unwrap()
                .layer_stack_data
                .clone();
            assert!(!stack.is_empty());
            for value in [0.5, 0.0, 1.0] {
                scene
                    .transaction()
                    .update_layer(FILTER, effect(value))
                    .commit()
                    .unwrap();
                assert!(update_materializer(&mut materializer, &scene));
                assert_eq!(
                    materializer
                        .canvas
                        .compiled_plan
                        .as_ref()
                        .unwrap()
                        .layer_stack_data,
                    stack
                );
                let changes = materializer.canvas.buffer_changes.as_ref().unwrap();
                assert!(changes.plan_structure_reused);
                assert!(
                    changes.plan_layer_stack.is_empty(),
                    "backdrop={backdrop}, empty={empty}: {:?}",
                    changes.plan_layer_stack
                );
            }
        }
    }
}

#[test]
fn empty_fused_groups_keep_real_stack_entries_when_updated() {
    for (before, after, expected) in [
        (opacity(0.25), opacity(0.75), 0.75),
        (blend(Mix::Multiply), blend(Mix::Screen), 1.0),
    ] {
        let mut scene = scene(before);
        scene.transaction().remove_subtree(FILTER).commit().unwrap();
        let mut materializer = PersistentSceneMaterializer::new(&scene);
        let original = materializer.canvas.compiled_plan.clone().unwrap();
        assert!(!original.layer_stack_locations.is_empty());
        scene
            .transaction()
            .update_layer(GROUP, after)
            .commit()
            .unwrap();
        assert!(update_materializer(&mut materializer, &scene));
        let plan = materializer.canvas.compiled_plan.as_ref().unwrap();
        assert!(Rc::ptr_eq(&original.draw_order, &plan.draw_order));
        assert_eq!(plan.layer_stack_locations, original.layer_stack_locations);
        assert!(
            !materializer
                .canvas
                .buffer_changes
                .as_ref()
                .unwrap()
                .plan_layer_stack
                .is_empty()
        );
        assert!(plan.layer_stack_data.iter().all(|entry| match entry {
            crate::shared::execution::LayerStackEntry::Opacity { opacity, .. } =>
                *opacity == expected,
            crate::shared::execution::LayerStackEntry::Blend { mode, .. } =>
                *mode == BlendMode::new(Mix::Screen, Compose::SrcOver),
            crate::shared::execution::LayerStackEntry::Clip { .. } => false,
        }));
    }
}

#[test]
fn layer_kind_changes_recompile_instead_of_claiming_a_descriptor_patch() {
    let mut scene = scene(opacity(0.25));
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let original = materializer.canvas.compiled_plan.clone().unwrap();
    scene
        .transaction()
        .update_layer(
            GROUP,
            RetainedLayerDescriptor::Isolate {
                path: Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
            },
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(matches!(
        root_layer(&materializer),
        OffscreenLayer::Isolate { .. }
    ));
    assert!(!Rc::ptr_eq(
        &original.draw_order,
        &materializer
            .canvas
            .compiled_plan
            .as_ref()
            .unwrap()
            .draw_order
    ));
}

#[test]
fn offscreen_layer_updates_reach_both_mask_branches() {
    for branch in [RetainedChildBranch::Content, RetainedChildBranch::Mask] {
        let mut scene = scene(opacity(0.25));
        let mask = RetainedNodeId::for_owner(71_004);
        let root = scene.root;
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(root),
                None,
                mask,
                RetainedLayerDescriptor::Mask(crate::Mask {
                    region: Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO),
                    kind: crate::MaskKind::Alpha,
                }),
            )
            .reparent(GROUP, RetainedParent { node: mask, branch }, None)
            .commit()
            .unwrap();
        let mut materializer = PersistentSceneMaterializer::new(&scene);
        scene
            .transaction()
            .update_layer(GROUP, opacity(0.75))
            .commit()
            .unwrap();
        assert!(update_materializer(&mut materializer, &scene));
        let plan = materializer.canvas.compiled_plan.as_ref().unwrap();
        let ExecOp::OffscreenMaskLayer { content, mask, .. } = &plan.ops[0] else {
            panic!("root mask operation")
        };
        let ops = if branch == RetainedChildBranch::Content {
            content
        } else {
            mask
        };
        assert!(matches!(
            &ops[0],
            ExecOp::OffscreenLayer {
                layer: OffscreenLayer::Opacity { opacity: 0.75, .. },
                ..
            }
        ));
    }
}
