//! Offscreen composites carry clips; opacity/blend ancestors own separate surfaces.
use super::*;
use crate::shared::bounds::Bounds;
use crate::shared::offscreen::local_offscreen_scene;
use peniko::{Compose, Mix, kurbo::Shape};

fn check_clips(plan: &ExecPlan, ops: &[ExecOp]) -> (usize, usize) {
    let mut count = 0;
    let mut depth = 0;
    for op in ops {
        let (stack, nested) = match op {
            ExecOp::OffscreenLayer {
                outer_stack,
                children,
                ..
            } => (outer_stack, check_clips(plan, children)),
            ExecOp::OffscreenMaskLayer {
                outer_stack,
                content,
                mask,
                ..
            } => {
                let a = check_clips(plan, content);
                let b = check_clips(plan, mask);
                (outer_stack, (a.0 + b.0, a.1.max(b.1)))
            }
            ExecOp::DrawBatch { .. } => continue,
        };
        assert!(
            plan.layer_stack_data[stack.clone()]
                .iter()
                .all(|entry| { matches!(entry, LayerStackEntry::Clip { .. }) }),
            "offscreen ancestors must be clips: {op:?}"
        );
        count += 1 + nested.0;
        depth = depth.max(stack.len()).max(nested.1);
    }
    (count, depth)
}

#[test]
fn clip_only_and_empty_clips_never_allocate_surfaces() {
    let rect = Rect::new(0.0, 0.0, 64.0, 64.0);
    for empty in [false, true] {
        for path in [false, true] {
            for depth in [1, 129] {
                let mut canvas = test_scene();
                for _ in 0..depth {
                    if path {
                        canvas.push_clip_layer(
                            rect.to_path(0.1),
                            Affine::translate((0.25, 0.25)),
                            FillRule::NonZero,
                            0.1,
                        );
                    } else {
                        canvas.push_clip_sdf_rect_layer(rect, Radius::ZERO);
                    }
                }
                if !empty {
                    canvas.push_rect(rect, Radius::ZERO, Color::WHITE);
                }
                for _ in 0..depth {
                    canvas.pop_layer();
                }
                assert_eq!(canvas.path_records.len(), if path { depth } else { 0 });
                let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
                assert_eq!(check_clips(&plan, &plan.ops), (0, 0));
                assert_eq!(plan.ops.len(), usize::from(!empty));
                assert_eq!(plan.layer_stack_data.len(), if empty { 0 } else { depth });
                let local = local_offscreen_scene(
                    &canvas,
                    &plan,
                    &plan.ops,
                    Bounds::new(-4, -6, 68, 70),
                    &(0..canvas.draw_records.len() as u32).collect::<Vec<_>>(),
                );
                assert_eq!(check_clips(&local.plan, &local.children), (0, 0));
                assert_eq!(local.children.len(), plan.ops.len());
            }
        }
    }
}

#[test]
fn offscreen_clips_survive_deep_groups_masks_backdrops_and_localization() {
    let rect = Rect::new(0.0, 0.0, 64.0, 64.0);
    for depth in [0, 1, 63, 64, 65, 128] {
        for kind in 0..4 {
            let mut canvas = test_scene();
            for index in 0..depth {
                if index % 2 == 0 {
                    canvas.push_clip_sdf_rect_layer(rect, Radius::ZERO);
                } else {
                    canvas.push_clip_layer(
                        rect.to_path(0.1),
                        Affine::IDENTITY,
                        FillRule::NonZero,
                        0.1,
                    );
                }
            }
            canvas.push_opacity_layer(rect.to_path(0.1), Affine::IDENTITY, 0.1, 0.5);
            canvas.push_blend_layer(
                rect.to_path(0.1),
                Affine::IDENTITY,
                0.1,
                Mix::Multiply,
                Compose::SrcOver,
            );
            let region = Region::rect(rect, Radius::ZERO);
            match kind {
                0 => canvas.push_filter_layer(Filter::Opacity(0.5), region),
                1 => canvas.push_backdrop_layer(Filter::Opacity(0.5), region),
                2 => canvas.push_isolate_layer(rect.to_path(0.1), Affine::IDENTITY, 0.1),
                _ => {
                    let mut mask = test_scene();
                    mask.push_rect(rect, Radius::ZERO, Color::WHITE);
                    canvas.push_mask_layer(
                        mask,
                        Mask {
                            region,
                            kind: crate::MaskKind::Alpha,
                        },
                    );
                }
            }
            // Fused groups still belong in draw-batch stacks inside the surface.
            canvas.push_opacity_layer(rect.to_path(0.1), Affine::IDENTITY, 0.1, 0.75);
            canvas.push_blend_layer(
                rect.to_path(0.1),
                Affine::IDENTITY,
                0.1,
                Mix::Screen,
                Compose::SrcOver,
            );
            canvas.push_rect(rect, Radius::ZERO, Color::WHITE);
            for _ in 0..depth + 5 {
                canvas.pop_layer();
            }
            let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
            assert_eq!(check_clips(&plan, &plan.ops), (3, depth));
            assert!(
                plan.layer_stack_data
                    .iter()
                    .any(|entry| matches!(entry, LayerStackEntry::Opacity { .. }))
            );
            assert!(
                plan.layer_stack_data
                    .iter()
                    .any(|entry| matches!(entry, LayerStackEntry::Blend { .. }))
            );
            let local = local_offscreen_scene(
                &canvas,
                &plan,
                &plan.ops,
                Bounds::new(4, 6, 56, 60),
                &(0..canvas.draw_records.len() as u32).collect::<Vec<_>>(),
            );
            assert_eq!(check_clips(&local.plan, &local.children), (3, depth));
        }
    }
}

#[test]
fn retained_offscreen_clips_survive_descriptor_topology_and_surface_updates() {
    use crate::retained_scene::{PersistentSceneMaterializer, RetainedLayerDescriptor as D};
    use crate::{RetainedNodeId, RetainedParent, RetainedScene};
    let id = RetainedNodeId::for_owner;
    let rect = Rect::new(0.0, 0.0, 64.0, 64.0);
    let clip = || D::ClipPath {
        path: rect.to_path(0.1),
        transform: Affine::IDENTITY,
        rule: FillRule::NonZero,
        tolerance: 0.1,
    };
    let opacity = |opacity| D::Opacity {
        path: rect.to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity,
    };
    let filter = || D::Filter {
        filter: Filter::Opacity(0.5),
        sample_region: Region::rect(rect, Radius::ZERO),
    };
    let mut scene = RetainedScene::new(64, 64, 1.0, id(91_000)).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(id(91_000)),
            None,
            id(91_001),
            clip(),
        )
        .insert_layer(
            RetainedParent::content(id(91_001)),
            None,
            id(91_002),
            opacity(0.5),
        )
        .insert_scene(
            RetainedParent::content(id(91_002)),
            None,
            id(91_003),
            SharedRc::new(test_scene()),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let check = |state: &PersistentSceneMaterializer| {
        let plan = state.canvas.compile(ROOT_COMMAND_LIST_ID);
        let fresh = PersistentSceneMaterializer::new(&scene);
        let expected = fresh.canvas.compile(ROOT_COMMAND_LIST_ID);
        assert_eq!(
            check_clips(&plan, &plan.ops),
            check_clips(&expected, &expected.ops)
        );
    };
    check(&state);
    // A previously fused ancestor becomes offscreen when a filter is inserted.
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(id(91_002)),
            None,
            id(91_004),
            filter(),
        )
        .reparent(id(91_003), RetainedParent::content(id(91_004)), None)
        .commit()
        .unwrap();
    state.update(&scene, scene.changes_since(state.version()));
    let verify = |state: &PersistentSceneMaterializer, scene: &RetainedScene| {
        let plan = state.canvas.compile(ROOT_COMMAND_LIST_ID);
        let fresh = PersistentSceneMaterializer::new(scene);
        let expected = fresh.canvas.compile(ROOT_COMMAND_LIST_ID);
        assert_eq!(
            check_clips(&plan, &plan.ops),
            check_clips(&expected, &expected.ops)
        );
        assert_eq!(check_clips(&plan, &plan.ops), (2, 1));
    };
    verify(&state, &scene);
    scene
        .transaction()
        .update_layer(id(91_001), clip())
        .update_layer(id(91_002), opacity(0.75))
        .commit()
        .unwrap();
    state.update(&scene, scene.changes_since(state.version()));
    verify(&state, &scene);
    scene
        .transaction()
        .resize(96, 80, 1.0)
        .set_transform(id(91_003), Affine::translate((4.0, 6.0)))
        .commit()
        .unwrap();
    state.update(&scene, scene.changes_since(state.version()));
    verify(&state, &scene);
    scene
        .transaction()
        .reparent(id(91_003), RetainedParent::content(id(91_002)), None)
        .remove_subtree(id(91_004))
        .commit()
        .unwrap();
    state.update(&scene, scene.changes_since(state.version()));
    let plan = state.canvas.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(check_clips(&plan, &plan.ops), (0, 0));
    assert!(
        plan.layer_stack_data
            .iter()
            .any(|entry| matches!(entry, LayerStackEntry::Opacity { .. }))
    );
}

#[test]
fn retained_offscreen_fragment_keeps_inherited_clips_when_children_change() {
    use crate::retained_scene::{PersistentSceneMaterializer, RetainedLayerDescriptor as D};
    use crate::{RetainedNodeId, RetainedParent, RetainedScene};
    let id = RetainedNodeId::for_owner;
    let rect = Rect::new(0.0, 0.0, 64.0, 64.0);
    for backdrop in [false, true] {
        let mut scene = RetainedScene::new(64, 64, 1.0, id(92_000)).unwrap();
        let filter = Filter::Opacity(0.5);
        let sample_region = Region::rect(rect, Radius::ZERO);
        let effect = if backdrop {
            D::Backdrop {
                filter,
                sample_region,
            }
        } else {
            D::Filter {
                filter,
                sample_region,
            }
        };
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(id(92_000)),
                None,
                id(92_001),
                D::ClipPath {
                    path: rect.to_path(0.1),
                    transform: Affine::IDENTITY,
                    rule: FillRule::NonZero,
                    tolerance: 0.1,
                },
            )
            .insert_layer(
                RetainedParent::content(id(92_001)),
                None,
                id(92_002),
                effect,
            )
            .commit()
            .unwrap();
        let mut state = PersistentSceneMaterializer::new(&scene);
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(id(92_002)),
                None,
                id(92_003),
                D::Isolate {
                    path: rect.to_path(0.1),
                    transform: Affine::IDENTITY,
                    tolerance: 0.1,
                },
            )
            .commit()
            .unwrap();
        state.update(&scene, scene.changes_since(state.version()));
        let plan = state.canvas.compile(ROOT_COMMAND_LIST_ID);
        let fresh = PersistentSceneMaterializer::new(&scene);
        let expected = fresh.canvas.compile(ROOT_COMMAND_LIST_ID);
        assert_eq!(
            check_clips(&plan, &plan.ops),
            check_clips(&expected, &expected.ops),
            "backdrop={backdrop}"
        );
        let ExecOp::OffscreenLayer {
            outer_stack,
            children,
            ..
        } = &plan.ops[0]
        else {
            unreachable!()
        };
        assert_eq!(outer_stack.len(), 1);
        let ExecOp::OffscreenLayer { outer_stack, .. } = &children[0] else {
            unreachable!()
        };
        assert_eq!(
            outer_stack.len(),
            usize::from(backdrop),
            "backdrop children inherit the ancestor clip"
        );
    }
}
