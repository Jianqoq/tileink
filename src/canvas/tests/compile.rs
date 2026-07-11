use super::*;
use crate::shared::layer::mask::MaskKind;
use peniko::{BlendMode, Compose, Mix};

#[test]
fn execution_plan_fingerprint_ignores_buffer_only_changes() {
    let build = |color| {
        let mut canvas = Canvas::new(32, 32, 1.0);
        canvas.push_rect(Rect::new(2.0, 2.0, 18.0, 18.0), crate::Radius::ZERO, color);
        canvas
    };
    let red = build(rgb(255, 0, 0));
    let green = build(rgb(0, 255, 0));
    assert_eq!(
        red.execution_plan_fingerprint(),
        green.execution_plan_fingerprint()
    );
}

#[test]
fn reset_discards_cached_execution_plan() {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        rgb(255, 0, 0),
    );
    canvas.compiled_plan = Some(canvas.compile_shared(ROOT_COMMAND_LIST_ID));

    canvas.reset();

    assert!(canvas.compile(ROOT_COMMAND_LIST_ID).draw_order.is_empty());
}

#[test]
fn reset_reuses_root_command_storage() {
    let mut canvas = test_scene();
    canvas.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    let capacity = canvas.command_lists[0].commands.capacity();
    let storage = canvas.command_lists[0].commands.as_ptr();
    assert!(capacity > 0);

    canvas.reset();

    assert_eq!(canvas.command_lists.len(), 1);
    assert!(canvas.command_lists[0].commands.is_empty());
    assert_eq!(canvas.command_lists[0].commands.capacity(), capacity);
    assert_eq!(canvas.command_lists[0].commands.as_ptr(), storage);
}

#[test]
fn compile_coalesces_plain_draws_across_materialized_retained_scenes() {
    let mut canvas = Canvas::new_retained(64, 64, 1.0, RetainedNodeId::for_owner(1));
    for owner in 2..=4 {
        let mut child = Canvas::new(16, 16, 1.0);
        child.push_rect(
            Rect::new(0.0, 0.0, 12.0, 12.0),
            Radius::ZERO,
            rgb(10 * owner as u8, 80, 160),
        );
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(owner),
            0,
            std::sync::Arc::new(child),
            (((owner - 2) * 16) as f64, 0.0),
        );
    }

    let canvas = canvas.materialize_retained_scenes(&mut RetainedSceneCache::default());
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    match plan.ops.as_slice() {
        [
            ExecOp::DrawBatch {
                draws, layer_stack, ..
            },
        ] => {
            assert_eq!(draws.as_ref().clone(), (0..3).collect::<Vec<_>>());
            assert!(layer_stack.is_empty());
        }
        ops => panic!("materialized retained scenes must not split GPU batches: {ops:#?}"),
    }
}

#[test]
fn compile_discards_orphaned_layer_stacks_after_retained_batch_coalescing() {
    let mut canvas = Canvas::new_retained(64, 64, 1.0, RetainedNodeId::for_owner(10));
    canvas.push_opacity_layer(rect_path(0.0, 0.0, 64.0, 64.0), Affine::IDENTITY, 0.1, 0.5);
    for owner in 11..=13 {
        let mut child = Canvas::new(16, 16, 1.0);
        child.push_rect(
            Rect::new(0.0, 0.0, 12.0, 12.0),
            Radius::ZERO,
            rgb(120, 80, 160),
        );
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(owner),
            0,
            std::sync::Arc::new(child),
            (((owner - 11) * 16) as f64, 0.0),
        );
    }
    canvas.pop_layer();

    let materialized = canvas.materialize_retained_scenes(&mut RetainedSceneCache::default());
    let plan = materialized.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(plan.layer_stack_data.len(), 1);
    assert_eq!(plan.draw_order.len(), 3);
}

#[test]
fn compile_preserves_stable_batch_for_empty_retained_layer_content() {
    let id = RetainedNodeId::for_owner(20);
    let owner = RetainedBatchOwner {
        node: id,
        branch: RetainedBatchBranch::Content,
    };
    let mut canvas = Canvas::new_retained(32, 32, 1.0, RetainedNodeId::for_owner(19));
    canvas.push_retained_opacity_layer(
        RetainedLayerKey::new(id, SceneRevision::INITIAL),
        rect_path(0.0, 0.0, 32.0, 32.0),
        Affine::IDENTITY,
        0.1,
        0.5,
    );
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    let batch = plan.retained_batch_ids[&owner];
    assert!(plan.ops.iter().any(|op| {
        matches!(
            op,
            ExecOp::DrawBatch { draws, batch_id, owners, .. }
                if draws.is_empty() && *batch_id == batch && owners.as_slice() == [owner]
        )
    }));
}

#[test]
fn compile_preserves_distinct_empty_batches_for_retained_mask_branches() {
    let id = RetainedNodeId::for_owner(22);
    let content = RetainedBatchOwner {
        node: id,
        branch: RetainedBatchBranch::Content,
    };
    let mask = RetainedBatchOwner {
        node: id,
        branch: RetainedBatchBranch::Mask,
    };
    let mut canvas = Canvas::new_retained(32, 32, 1.0, RetainedNodeId::for_owner(21));
    canvas.push_retained_mask_layer(
        RetainedLayerKey::new(id, SceneRevision::INITIAL),
        Canvas::new(32, 32, 1.0),
        Mask {
            region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
            kind: MaskKind::Alpha,
        },
    );
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    let content_batch = plan.retained_batch_ids[&content];
    let mask_batch = plan.retained_batch_ids[&mask];
    assert_ne!(content_batch, mask_batch);
    for (owner, batch) in [(content, content_batch), (mask, mask_batch)] {
        assert!(plan.ops.iter().any(|op| {
            match op {
            ExecOp::OffscreenMaskLayer {
                content,
                mask,
                ..
            } => content.iter().chain(mask).any(|op| {
                matches!(
                    op,
                    ExecOp::DrawBatch { draws, batch_id, owners, .. }
                        if draws.is_empty() && *batch_id == batch && owners.as_slice() == [owner]
                )
            }),
            _ => false,
        }
        }));
    }
}

#[test]
fn compile_lowers_clip_blend_batches_in_user_order() {
    let mut canvas = test_scene();
    canvas.push_clip_layer(
        rect_path(0.0, 0.0, 32.0, 32.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );
    canvas.push_path(
        rect_path(2.0, 2.0, 8.0, 8.0),
        Brush::Solid(rgb(255, 0, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );
    canvas.push_blend_layer(
        rect_path(4.0, 4.0, 24.0, 24.0),
        Affine::IDENTITY,
        0.25,
        Mix::Multiply,
        Compose::SrcOver,
    );
    canvas.push_path(
        rect_path(6.0, 6.0, 12.0, 12.0),
        Brush::Solid(rgb(0, 255, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );
    canvas.pop_layer();
    canvas.push_path(
        rect_path(10.0, 10.0, 18.0, 18.0),
        Brush::Solid(rgb(0, 0, 255)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(plan.ops.len(), 7, "{:#?}", plan.ops);

    match &plan.ops[0] {
        ExecOp::BeginClip => {}
        op => panic!("expected BeginClip, got {op:#?}"),
    }
    match &plan.ops[1] {
        ExecOp::DrawBatch {
            draws, layer_stack, ..
        } => {
            assert_eq!(draws.as_ref().clone(), (1..2).collect::<Vec<_>>());
            assert_layer_stack(
                &plan,
                layer_stack.clone(),
                &[LayerStackEntry::Clip { draw: 0 }],
            );
        }
        op => panic!("expected first DrawBatch, got {op:#?}"),
    }
    match &plan.ops[2] {
        ExecOp::BeginBlend => {}
        op => panic!("expected BeginBlend, got {op:#?}"),
    }
    match &plan.ops[3] {
        ExecOp::DrawBatch {
            draws, layer_stack, ..
        } => {
            assert_eq!(draws.as_ref().clone(), (3..4).collect::<Vec<_>>());
            assert_layer_stack(
                &plan,
                layer_stack.clone(),
                &[
                    LayerStackEntry::Clip { draw: 0 },
                    LayerStackEntry::Blend {
                        draw: 2,
                        mode: BlendMode::new(Mix::Multiply, Compose::SrcOver),
                    },
                ],
            );
        }
        op => panic!("expected second DrawBatch, got {op:#?}"),
    }
    match &plan.ops[4] {
        ExecOp::EndBlend => {}
        op => panic!("expected EndBlend, got {op:#?}"),
    }
    match &plan.ops[5] {
        ExecOp::DrawBatch {
            draws, layer_stack, ..
        } => {
            assert_eq!(draws.as_ref().clone(), (4..5).collect::<Vec<_>>());
            assert_layer_stack(
                &plan,
                layer_stack.clone(),
                &[LayerStackEntry::Clip { draw: 0 }],
            );
        }
        op => panic!("expected third DrawBatch, got {op:#?}"),
    }
    match &plan.ops[6] {
        ExecOp::EndClip => {}
        op => panic!("expected EndClip, got {op:#?}"),
    }
}

#[test]
fn compile_keeps_opacity_group_alive_across_nested_batches() {
    let mut canvas = test_scene();
    canvas.push_opacity_layer(rect_path(0.0, 0.0, 32.0, 32.0), Affine::IDENTITY, 0.25, 0.5);
    canvas.push_path(
        rect_path(2.0, 2.0, 8.0, 8.0),
        Brush::Solid(rgb(255, 0, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );
    canvas.push_blend_layer(
        rect_path(4.0, 4.0, 24.0, 24.0),
        Affine::IDENTITY,
        0.25,
        Mix::Screen,
        Compose::SrcOver,
    );
    canvas.push_path(
        rect_path(6.0, 6.0, 12.0, 12.0),
        Brush::Solid(rgb(0, 255, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );
    canvas.pop_layer();
    canvas.push_path(
        rect_path(10.0, 10.0, 18.0, 18.0),
        Brush::Solid(rgb(0, 0, 255)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.25,
    );
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(plan.ops.len(), 7, "{:#?}", plan.ops);

    match &plan.ops[0] {
        ExecOp::BeginOpacity => {}
        op => panic!("expected BeginOpacity, got {op:#?}"),
    }
    match &plan.ops[1] {
        ExecOp::DrawBatch {
            draws, layer_stack, ..
        } => {
            assert_eq!(draws.as_ref().clone(), (1..2).collect::<Vec<_>>());
            assert_layer_stack(
                &plan,
                layer_stack.clone(),
                &[LayerStackEntry::Opacity {
                    draw: 0,
                    opacity: 0.5,
                }],
            );
        }
        op => panic!("expected first DrawBatch, got {op:#?}"),
    }
    match &plan.ops[2] {
        ExecOp::BeginBlend => {}
        op => panic!("expected BeginBlend, got {op:#?}"),
    }
    match &plan.ops[3] {
        ExecOp::DrawBatch {
            draws, layer_stack, ..
        } => {
            assert_eq!(draws.as_ref().clone(), (3..4).collect::<Vec<_>>());
            assert_layer_stack(
                &plan,
                layer_stack.clone(),
                &[
                    LayerStackEntry::Opacity {
                        draw: 0,
                        opacity: 0.5,
                    },
                    LayerStackEntry::Blend {
                        draw: 2,
                        mode: BlendMode::new(Mix::Screen, Compose::SrcOver),
                    },
                ],
            );
        }
        op => panic!("expected second DrawBatch, got {op:#?}"),
    }
    match &plan.ops[4] {
        ExecOp::EndBlend => {}
        op => panic!("expected EndBlend, got {op:#?}"),
    }
    match &plan.ops[5] {
        ExecOp::DrawBatch {
            draws, layer_stack, ..
        } => {
            assert_eq!(draws.as_ref().clone(), (4..5).collect::<Vec<_>>());
            assert_layer_stack(
                &plan,
                layer_stack.clone(),
                &[LayerStackEntry::Opacity {
                    draw: 0,
                    opacity: 0.5,
                }],
            );
        }
        op => panic!("expected third DrawBatch, got {op:#?}"),
    }
    match &plan.ops[6] {
        ExecOp::EndOpacity => {}
        op => panic!("expected EndOpacity, got {op:#?}"),
    }
}

#[test]
fn compile_fuses_sdf_clip_into_layer_stack() {
    let mut canvas = test_scene();
    canvas.push_clip_sdf_rect_layer(Rect::new(4.0, 4.0, 32.0, 32.0), Radius::all(6.0));
    canvas.push_path(
        rect_path(0.0, 0.0, 40.0, 40.0),
        Brush::Solid(rgb(255, 0, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(canvas.draw_records.len(), 2);
    match canvas.draw_sdf(&canvas.draw_records[0]) {
        Some(Sdf::Rect(rect)) => assert_eq!(rect.radius.top_left, 6.0),
        sdf => panic!("expected hidden SDF clip draw, got {sdf:#?}"),
    }
    assert_eq!(plan.ops.len(), 3, "{:#?}", plan.ops);
    match &plan.ops[0] {
        ExecOp::BeginClip => {}
        op => panic!("expected BeginClip, got {op:#?}"),
    }
    match &plan.ops[1] {
        ExecOp::DrawBatch {
            draws, layer_stack, ..
        } => {
            assert_eq!(draws.as_ref().clone(), (1..2).collect::<Vec<_>>());
            assert_layer_stack(
                &plan,
                layer_stack.clone(),
                &[LayerStackEntry::Clip { draw: 0 }],
            );
        }
        op => panic!("expected clipped DrawBatch, got {op:#?}"),
    }
    match &plan.ops[2] {
        ExecOp::EndClip => {}
        op => panic!("expected EndClip, got {op:#?}"),
    }
}

#[test]
fn path_api_promotes_axis_aligned_rect_clip_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_clip_layer(
        rect_path(1.0, 2.0, 17.0, 18.0),
        Affine::translate((3.0, 4.0)),
        FillRule::EvenOdd,
        0.1,
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 32.0, 32.0),
        Radius::ZERO,
        rgb(255, 0, 0),
    );
    canvas.pop_layer();

    assert!(canvas.path_records.is_empty());
    match canvas.draw_sdf(&canvas.draw_records[0]) {
        Some(Sdf::Rect(rect)) => {
            assert_eq!(rect.start, Point::new(4.0, 6.0));
            assert_eq!(rect.end, Point::new(20.0, 22.0));
            assert!(rect.radius.is_zero());
        }
        sdf => panic!("expected promoted rectangular clip, got {sdf:#?}"),
    }
}

#[test]
fn path_api_keeps_non_rectangular_clip_as_path_geometry() {
    let mut triangle = BezPath::new();
    triangle.move_to((2.0, 2.0));
    triangle.line_to((30.0, 4.0));
    triangle.line_to((12.0, 28.0));
    triangle.close_path();

    let mut canvas = test_scene();
    canvas.push_clip_layer(triangle, Affine::IDENTITY, FillRule::NonZero, 0.1);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 32.0, 32.0),
        Radius::ZERO,
        rgb(255, 0, 0),
    );
    canvas.pop_layer();

    assert_eq!(canvas.path_records.len(), 1);
    assert!(canvas.draw_sdf(&canvas.draw_records[0]).is_none());
}

#[test]
fn path_api_keeps_fractional_physical_rect_clip_as_path_geometry() {
    let mut canvas = Canvas::new(64, 64, 1.5);
    canvas.push_clip_layer(
        rect_path(1.0, 1.0, 17.0, 17.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 32.0, 32.0),
        Radius::ZERO,
        rgb(255, 0, 0),
    );
    canvas.pop_layer();

    // Path and SDF coverage differ at fractional physical clip edges. Keep the
    // path so DPI/viewBox scaling remains pixel-identical to existing scenes.
    assert_eq!(canvas.path_records.len(), 1);
    assert!(canvas.draw_sdf(&canvas.draw_records[0]).is_none());
}

#[test]
fn compile_fuses_generic_sdf_clip_without_path_storage() {
    let mut canvas = test_scene();
    canvas.push_clip_sdf_layer(Sdf::Line(SdfLine::new(
        Point::new(8.0, 24.0),
        Point::new(40.0, 24.0),
        6.0,
        crate::shared::sdf::line::LineCap::Round,
    )));
    canvas.push_rect(
        Rect::new(0.0, 0.0, 48.0, 48.0),
        Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert!(canvas.path_records.is_empty());
    assert!(canvas.path_records.is_empty());
    match canvas.draw_sdf(&canvas.draw_records[0]) {
        Some(Sdf::Line(line)) => assert_eq!(line.width, 6.0),
        sdf => panic!("expected hidden line SDF clip draw, got {sdf:#?}"),
    }
    assert_eq!(canvas.draw_records[0].pixel_bounds.x0, 5);
    assert_eq!(canvas.draw_records[0].pixel_bounds.y0, 21);
    assert_eq!(canvas.draw_records[0].pixel_bounds.x1, 43);
    assert_eq!(canvas.draw_records[0].pixel_bounds.y1, 27);
    assert_eq!(plan.ops.len(), 3, "{:#?}", plan.ops);
    match &plan.ops[1] {
        ExecOp::DrawBatch {
            draws, layer_stack, ..
        } => {
            assert_eq!(draws.as_ref().clone(), (1..2).collect::<Vec<_>>());
            assert_layer_stack(
                &plan,
                layer_stack.clone(),
                &[LayerStackEntry::Clip { draw: 0 }],
            );
        }
        op => panic!("expected clipped DrawBatch, got {op:#?}"),
    }
}

#[test]
fn compile_keeps_outer_sdf_clip_on_backdrop_layer_children() {
    let mut canvas = test_scene();
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::all(12.0));
    canvas.push_backdrop_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    canvas.pop_layer();
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    match &plan.ops[..] {
        [
            ExecOp::BeginClip,
            ExecOp::OffscreenLayer {
                outer_stack,
                children,
                ..
            },
            ExecOp::EndClip,
        ] => {
            assert_layer_stack(
                &plan,
                outer_stack.clone(),
                &[LayerStackEntry::Clip { draw: 0 }],
            );
            match &children[..] {
                [
                    ExecOp::DrawBatch {
                        draws, layer_stack, ..
                    },
                ] => {
                    assert_eq!(draws.as_ref().clone(), (1..2).collect::<Vec<_>>());
                    assert_layer_stack(
                        &plan,
                        layer_stack.clone(),
                        &[LayerStackEntry::Clip { draw: 0 }],
                    );
                }
                ops => panic!("expected backdrop child draw batch, got {ops:#?}"),
            }
        }
        ops => panic!("expected clipped backdrop layer, got {ops:#?}"),
    }
}

#[test]
fn compile_keeps_opacity_with_offscreen_child_isolated() {
    let mut canvas = test_scene();
    canvas.push_opacity_layer(rect_path(0.0, 0.0, 48.0, 48.0), Affine::IDENTITY, 0.0, 0.5);
    canvas.push_filter_layer(
        Filter::Opacity(1.0),
        Region::rect(Rect::new(0.0, 0.0, 48.0, 48.0), Radius::ZERO),
    );
    canvas.push_path(
        rect_path(8.0, 8.0, 40.0, 40.0),
        Brush::Solid(rgb(0, 0, 255)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    canvas.pop_layer();
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(plan.ops.len(), 1, "{:#?}", plan.ops);
    match &plan.ops[0] {
        ExecOp::OffscreenLayer {
            retained_id: _,
            draw,
            layer: Layer::Opacity(opacity),
            outer_stack,
            children,
        } => {
            assert_eq!(*draw, 0);
            assert_eq!(opacity.opacity, 0.5);
            assert!(outer_stack.is_empty());
            assert!(matches!(
                children.as_slice(),
                [ExecOp::OffscreenLayer {
                    layer: Layer::Filter { .. },
                    ..
                }]
            ));
        }
        op => panic!("expected isolated opacity offscreen layer, got {op:#?}"),
    }
}

#[test]
fn compile_keeps_blend_with_offscreen_child_isolated() {
    let mut canvas = test_scene();
    canvas.push_blend_layer(
        rect_path(0.0, 0.0, 48.0, 48.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    canvas.push_filter_layer(
        Filter::Opacity(1.0),
        Region::rect(Rect::new(0.0, 0.0, 48.0, 48.0), Radius::ZERO),
    );
    canvas.push_path(
        rect_path(8.0, 8.0, 40.0, 40.0),
        Brush::Solid(rgb(0, 255, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    canvas.pop_layer();
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(plan.ops.len(), 1, "{:#?}", plan.ops);
    match &plan.ops[0] {
        ExecOp::OffscreenLayer {
            retained_id: _,
            draw,
            layer: Layer::Blend(blend),
            outer_stack,
            children,
        } => {
            assert_eq!(*draw, 0);
            assert_eq!(blend.mode, BlendMode::new(Mix::Multiply, Compose::SrcOver));
            assert!(outer_stack.is_empty());
            assert!(matches!(
                children.as_slice(),
                [ExecOp::OffscreenLayer {
                    layer: Layer::Filter { .. },
                    ..
                }]
            ));
        }
        op => panic!("expected isolated blend offscreen layer, got {op:#?}"),
    }
}

#[test]
fn compile_keeps_isolate_as_offscreen_layer() {
    let mut canvas = test_scene();
    canvas.push_isolate_layer(rect_path(0.0, 0.0, 48.0, 48.0), Affine::IDENTITY, 0.0);
    canvas.push_path(
        rect_path(8.0, 8.0, 40.0, 40.0),
        Brush::Solid(rgb(255, 0, 0)),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(plan.ops.len(), 1, "{:#?}", plan.ops);
    match &plan.ops[0] {
        ExecOp::OffscreenLayer {
            retained_id: _,
            draw,
            layer: Layer::Isolate,
            outer_stack,
            children,
        } => {
            assert_eq!(*draw, 0);
            assert!(outer_stack.is_empty());
            match children.as_slice() {
                [
                    ExecOp::DrawBatch {
                        draws, layer_stack, ..
                    },
                ] => {
                    assert_eq!(draws.as_ref().clone(), (1..2).collect::<Vec<_>>());
                    assert!(layer_stack.is_empty());
                }
                ops => panic!("expected one isolate child batch, got {ops:#?}"),
            }
        }
        op => panic!("expected isolate offscreen layer, got {op:#?}"),
    }
}

#[test]
fn compile_keeps_mask_content_and_mask_isolated() {
    let mut canvas = test_scene();
    let mut mask_scene = test_scene();
    mask_scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 64.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 255, 255)),
    );
    canvas.push_mask_layer(
        mask_scene,
        Mask {
            region: Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO),
            kind: MaskKind::Alpha,
        },
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        crate::Radius::ZERO,
        Brush::Solid(rgb(255, 0, 0)),
    );
    canvas.pop_layer();

    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(plan.ops.len(), 1, "{:#?}", plan.ops);
    match &plan.ops[0] {
        ExecOp::OffscreenMaskLayer {
            retained_id: _,
            layer,
            outer_stack,
            content,
            mask,
        } => {
            assert!(outer_stack.is_empty());
            assert_eq!(layer.kind, MaskKind::Alpha);
            match content.as_slice() {
                [
                    ExecOp::DrawBatch {
                        draws, layer_stack, ..
                    },
                ] => {
                    assert_eq!(draws.as_ref().clone(), (1..2).collect::<Vec<_>>());
                    assert!(layer_stack.is_empty());
                }
                ops => panic!("expected one mask content batch, got {ops:#?}"),
            }
            match mask.as_slice() {
                [
                    ExecOp::DrawBatch {
                        draws, layer_stack, ..
                    },
                ] => {
                    assert_eq!(draws.as_ref().clone(), (0..1).collect::<Vec<_>>());
                    assert!(layer_stack.is_empty());
                }
                ops => panic!("expected one mask source batch, got {ops:#?}"),
            }
        }
        op => panic!("expected mask offscreen layer, got {op:#?}"),
    }
}
