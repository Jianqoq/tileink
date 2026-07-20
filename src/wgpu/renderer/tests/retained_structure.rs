use super::*;
use crate::wgpu::renderer::ExternalTextureHistoryId;

#[test]
fn persistent_external_texture_repaints_a_reinserted_scene() {
    if !run_wgpu_tests() {
        return;
    }

    const SIZE: (u32, u32) = (64, 64);
    let root = RetainedNodeId::for_owner(50_220);
    let filtered = RetainedNodeId::for_owner(50_221);
    let background = RetainedNodeId::for_owner(50_222);
    let popup = RetainedNodeId::for_owner(50_223);
    let animated = RetainedNodeId::for_owner(50_224);
    let solid = |rect: Rect, color| {
        let mut canvas = Canvas::new(SIZE.0, SIZE.1, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let background_canvas = solid(
        Rect::new(0.0, 0.0, SIZE.0 as f64, SIZE.1 as f64),
        Color::from_rgb8(20, 30, 40),
    );
    let popup_canvas = solid(
        Rect::new(0.0, 0.0, 20.0, 16.0),
        Color::from_rgb8(230, 240, 250),
    );
    let popup_transform = Affine::translate((24.0, 20.0));
    let mut scene = RetainedScene::new(SIZE.0, SIZE.1, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            filtered,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(0.0, 0.0, SIZE.0 as f64, SIZE.1 as f64).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 1.0,
            },
        )
        .insert_scene(
            RetainedParent::content(filtered),
            None,
            background,
            background_canvas,
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            popup,
            popup_canvas.clone(),
            popup_transform,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            animated,
            solid(Rect::new(0.0, 0.0, 8.0, 8.0), Color::from_rgb8(80, 90, 100)),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(SIZE.0, SIZE.1, Color::TRANSPARENT);
    let target = external_target(
        incremental.device(),
        SIZE,
        "reinserted scene external history",
    );
    incremental
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &target,
            ExternalTextureHistoryId::new(220),
        )
        .unwrap();
    scene
        .transaction()
        .remove_subtree(popup)
        .replace_scene(
            animated,
            solid(Rect::new(0.0, 0.0, 8.0, 8.0), Color::from_rgb8(81, 90, 100)),
        )
        .commit()
        .unwrap();
    incremental
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &target,
            ExternalTextureHistoryId::new(220),
        )
        .unwrap();
    scene
        .transaction()
        .replace_scene(
            animated,
            solid(Rect::new(0.0, 0.0, 8.0, 8.0), Color::from_rgb8(82, 90, 100)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            popup,
            popup_canvas,
            popup_transform,
        )
        .commit()
        .unwrap();
    incremental
        .render_retained_to_persistent_wgpu_texture(
            &scene,
            &target,
            ExternalTextureHistoryId::new(220),
        )
        .unwrap();
    assert!(!incremental.incremental_render_stats().full_redraw);

    let mut full = new_test_renderer(SIZE.0, SIZE.1, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    let expected = external_target(full.device(), SIZE, "reinserted scene full reference");
    full.render_retained_to_persistent_wgpu_texture(
        &scene,
        &expected,
        ExternalTextureHistoryId::new(221),
    )
    .unwrap();

    assert_eq!(
        read_texture_rgba8(
            incremental.device(),
            incremental.queue(),
            &target,
            SIZE.0,
            SIZE.1,
        ),
        read_texture_rgba8(full.device(), full.queue(), &expected, SIZE.0, SIZE.1),
    );
}

#[test]
fn persistent_retained_nested_layer_insert_rebuilds_only_offscreen_ancestor() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_230);
    let filter = RetainedNodeId::for_owner(50_231);
    let base = RetainedNodeId::for_owner(50_232);
    let nested = RetainedNodeId::for_owner(50_233);
    let leaf = RetainedNodeId::for_owner(50_234);
    let region = Region::rect(Rect::new(0.0, 0.0, 48.0, 16.0), crate::Radius::ZERO);
    let solid = |rect: Rect, color| {
        let mut canvas = Canvas::new(48, 16, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(48, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            filter,
            RetainedLayerDescriptor::Filter {
                filter: Filter::Opacity(0.75),
                sample_region: region,
            },
        )
        .insert_scene(
            RetainedParent::content(filter),
            None,
            base,
            solid(
                Rect::new(0.0, 0.0, 28.0, 16.0),
                Color::from_rgb8(30, 70, 180),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(48, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    let initial = renderer.image();

    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(filter),
            None,
            nested,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(12.0, 0.0, 44.0, 16.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.5,
            },
        )
        .insert_scene(
            RetainedParent::content(nested),
            None,
            leaf,
            solid(
                Rect::new(12.0, 0.0, 44.0, 16.0),
                Color::from_rgb8(240, 80, 30),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    assert_eq!(renderer.incremental_render_stats().chunks_rebuilt, 2);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        3
    );

    let incremental = renderer.image();
    let mut full = new_test_renderer(48, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.pixels, full.image().pixels);

    scene.transaction().remove_subtree(nested).commit().unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        3
    );
    assert_eq!(renderer.image().pixels, initial.pixels);
}

#[test]
fn persistent_retained_nested_mask_branch_insert_rebuilds_only_mask_ancestor() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_240);
    let mask_layer = RetainedNodeId::for_owner(50_241);
    let content = RetainedNodeId::for_owner(50_242);
    let nested = RetainedNodeId::for_owner(50_243);
    let mask_leaf = RetainedNodeId::for_owner(50_244);
    let solid = |rect: Rect, color| {
        let mut canvas = Canvas::new(32, 16, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            mask_layer,
            RetainedLayerDescriptor::Mask(Mask {
                region: Region::rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO),
                kind: MaskKind::Alpha,
            }),
        )
        .insert_scene(
            RetainedParent::content(mask_layer),
            None,
            content,
            solid(
                Rect::new(0.0, 0.0, 32.0, 16.0),
                Color::from_rgb8(30, 70, 180),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    let initial = renderer.image();

    scene
        .transaction()
        .insert_layer(
            RetainedParent::mask(mask_layer),
            None,
            nested,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(8.0, 0.0, 24.0, 16.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 1.0,
            },
        )
        .insert_scene(
            RetainedParent::content(nested),
            None,
            mask_leaf,
            solid(Rect::new(8.0, 0.0, 24.0, 16.0), Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        3
    );

    let incremental = renderer.image();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.pixels, full.image().pixels);

    scene.transaction().remove_subtree(nested).commit().unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        3
    );
    assert_eq!(renderer.image().pixels, initial.pixels);
}

#[test]
fn persistent_retained_nested_layer_reorder_rebuilds_one_offscreen_ancestor() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_245);
    let filter = RetainedNodeId::for_owner(50_246);
    let back_layer = RetainedNodeId::for_owner(50_247);
    let back = RetainedNodeId::for_owner(50_248);
    let front_layer = RetainedNodeId::for_owner(50_249);
    let front = RetainedNodeId::for_owner(50_250);
    let opacity = RetainedLayerDescriptor::Opacity {
        path: Rect::new(0.0, 0.0, 32.0, 16.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity: 0.75,
    };
    let solid = |color| {
        let mut canvas = Canvas::new(32, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            filter,
            RetainedLayerDescriptor::Filter {
                filter: Filter::Opacity(1.0),
                sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO),
            },
        )
        .insert_layer(
            RetainedParent::content(filter),
            None,
            back_layer,
            opacity.clone(),
        )
        .insert_scene(
            RetainedParent::content(back_layer),
            None,
            back,
            solid(Color::from_rgb8(220, 30, 40)),
            Affine::translate((0.0, 0.0)),
        )
        .insert_layer(RetainedParent::content(filter), None, front_layer, opacity)
        .insert_scene(
            RetainedParent::content(front_layer),
            None,
            front,
            solid(Color::from_rgb8(30, 60, 220)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);

    scene
        .transaction()
        .move_before(front_layer, back_layer)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    assert_eq!(renderer.incremental_render_stats().chunks_rebuilt, 0);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        3
    );

    let incremental = renderer.image();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.pixels, full.image().pixels);
}

#[test]
fn persistent_retained_layer_reparent_rebuilds_old_and_new_offscreen_ancestors() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_255);
    let first_filter = RetainedNodeId::for_owner(50_256);
    let second_filter = RetainedNodeId::for_owner(50_257);
    let moving_layer = RetainedNodeId::for_owner(50_258);
    let leaf = RetainedNodeId::for_owner(50_259);
    let filter = |opacity| RetainedLayerDescriptor::Filter {
        filter: Filter::Opacity(opacity),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO),
    };
    let mut child = Canvas::new(32, 16, 1.0);
    child.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(220, 30, 40),
    );
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            first_filter,
            filter(0.75),
        )
        .insert_layer(
            RetainedParent::content(first_filter),
            None,
            moving_layer,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(0.0, 0.0, 32.0, 16.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.5,
            },
        )
        .insert_scene(
            RetainedParent::content(moving_layer),
            None,
            leaf,
            std::rc::Rc::new(child),
            Affine::translate((0.0, 0.0)),
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            second_filter,
            filter(0.25),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);

    scene
        .transaction()
        .reparent(moving_layer, RetainedParent::content(second_filter), None)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    assert_eq!(renderer.incremental_render_stats().chunks_rebuilt, 0);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        4
    );

    let incremental = renderer.image();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.pixels, full.image().pixels);
}

#[test]
fn persistent_root_offscreen_layer_reorder_reuses_child_fragments() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_265);
    let back_filter = RetainedNodeId::for_owner(50_266);
    let back = RetainedNodeId::for_owner(50_267);
    let front_filter = RetainedNodeId::for_owner(50_268);
    let front = RetainedNodeId::for_owner(50_269);
    let descriptor = RetainedLayerDescriptor::Filter {
        filter: Filter::Opacity(0.75),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO),
    };
    let solid = |color| {
        let mut canvas = Canvas::new(32, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            back_filter,
            descriptor.clone(),
        )
        .insert_scene(
            RetainedParent::content(back_filter),
            None,
            back,
            solid(Color::from_rgb8(220, 30, 40)),
            Affine::translate((0.0, 0.0)),
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            front_filter,
            descriptor,
        )
        .insert_scene(
            RetainedParent::content(front_filter),
            None,
            front,
            solid(Color::from_rgb8(30, 60, 220)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);

    scene
        .transaction()
        .move_before(front_filter, back_filter)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    assert_eq!(renderer.incremental_render_stats().chunks_rebuilt, 0);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        2
    );

    let incremental = renderer.image();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.pixels, full.image().pixels);
}

#[test]
fn persistent_retained_layer_reorder_reuses_plan_and_matches_force_full() {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(50_250);
    let layer = RetainedNodeId::for_owner(50_251);
    let back = RetainedNodeId::for_owner(50_252);
    let front = RetainedNodeId::for_owner(50_253);
    let child = |color| {
        let mut canvas = Canvas::new(32, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(0.0, 0.0, 32.0, 16.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.5,
            },
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            back,
            child(Color::from_rgb8(220, 30, 40)),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            front,
            child(Color::from_rgb8(30, 60, 220)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut incremental = new_test_renderer(32, 16, Color::TRANSPARENT);
    incremental.render_retained(&scene);

    scene
        .transaction()
        .move_before(front, back)
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    assert!(incremental.incremental_render_stats().reused_compiled_plan);
    assert_eq!(incremental.incremental_render_stats().chunks_rebuilt, 0);

    let reference = scene.to_canvas();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&reference);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn persistent_retained_layer_add_remove_reuses_empty_batch_context() {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(50_260);
    let layer = RetainedNodeId::for_owner(50_261);
    let extra = RetainedNodeId::for_owner(50_263);
    let child = |color| {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(0.0, 0.0, 32.0, 16.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.5,
            },
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);

    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(layer),
            None,
            extra,
            child(Color::from_rgb8(30, 210, 70)),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(renderer.incremental_render_stats().reused_compiled_plan);
    assert_eq!(renderer.incremental_render_stats().root_draw_batches, 1);
    assert_eq!(renderer.image().rgba8_at(24, 8), [15, 105, 35, 128]);

    scene.transaction().remove_subtree(extra).commit().unwrap();
    renderer.render_retained(&scene);
    assert!(renderer.incremental_render_stats().reused_compiled_plan);
    assert_eq!(renderer.incremental_render_stats().root_draw_batches, 0);
    assert_eq!(renderer.image().rgba8_at(24, 8), [0, 0, 0, 0]);

    let reference = scene.to_canvas();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&reference);
    assert_eq!(renderer.image().pixels, full.image().pixels);
}

#[test]
fn persistent_retained_reparent_between_layers_reuses_stable_batches() {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(50_270);
    let first_layer = RetainedNodeId::for_owner(50_271);
    let second_layer = RetainedNodeId::for_owner(50_272);
    let moving = RetainedNodeId::for_owner(50_273);
    let child = |color| {
        let mut canvas = Canvas::new(32, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let opacity = |opacity| RetainedLayerDescriptor::Opacity {
        path: Rect::new(0.0, 0.0, 32.0, 16.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity,
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            first_layer,
            opacity(0.5),
        )
        .insert_scene(
            RetainedParent::content(first_layer),
            None,
            moving,
            child(Color::from_rgb8(220, 30, 40)),
            Affine::translate((0.0, 0.0)),
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            second_layer,
            opacity(0.25),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);

    scene
        .transaction()
        .reparent(moving, RetainedParent::content(second_layer), None)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(renderer.incremental_render_stats().reused_compiled_plan);
    assert_eq!(renderer.incremental_render_stats().root_draw_batches, 1);

    // The moved leaf must retain a command location for its next independent paint update.
    scene
        .transaction()
        .replace_scene(moving, child(Color::from_rgb8(30, 210, 70)))
        .commit()
        .unwrap();
    renderer.render_retained(&scene);

    let reference = scene.to_canvas();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&reference);
    assert_eq!(renderer.image().pixels, full.image().pixels);
}

#[test]
fn persistent_retained_reparent_across_mask_branches_reuses_plan() {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(50_280);
    let mask_layer = RetainedNodeId::for_owner(50_281);
    let content_anchor = RetainedNodeId::for_owner(50_282);
    let moving = RetainedNodeId::for_owner(50_283);
    let child = |color| {
        let mut canvas = Canvas::new(32, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            mask_layer,
            RetainedLayerDescriptor::Mask(Mask {
                region: Region::rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO),
                kind: MaskKind::Alpha,
            }),
        )
        .insert_scene(
            RetainedParent::content(mask_layer),
            None,
            content_anchor,
            child(Color::from_rgb8(30, 60, 220)),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(mask_layer),
            None,
            moving,
            child(Color::WHITE),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);

    scene
        .transaction()
        .reparent(moving, RetainedParent::mask(mask_layer), None)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(renderer.incremental_render_stats().reused_compiled_plan);

    let reference = scene.to_canvas();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&reference);
    assert_eq!(renderer.image().pixels, full.image().pixels);
}

#[test]
fn persistent_retained_scene_add_remove_reuses_pages_without_ghost_draws() {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(50_300);
    let base = RetainedNodeId::for_owner(50_301);
    let extra = RetainedNodeId::for_owner(50_302);
    let child = |color| {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            child(Color::from_rgb8(220, 30, 40)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);

    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            extra,
            child(Color::from_rgb8(30, 210, 70)),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(24, 8), [30, 210, 70, 255]);

    scene.transaction().remove_subtree(extra).commit().unwrap();
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(24, 8), [0, 0, 0, 0]);

    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            extra,
            child(Color::from_rgb8(20, 80, 230)),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    let reference = scene.to_canvas();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&reference);
    assert_eq!(renderer.image().pixels, full.image().pixels);
}

#[test]
fn persistent_plain_leaf_can_update_after_topology_fast_path_insertion() {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(50_303);
    let base = RetainedNodeId::for_owner(50_304);
    let inserted = RetainedNodeId::for_owner(50_305);
    let child = |color| {
        let mut canvas = Canvas::new(32, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            child(Color::from_rgb8(220, 30, 40)),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut incremental = new_test_renderer(32, 16, Color::TRANSPARENT);
    incremental.render_retained(&scene);

    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            inserted,
            child(Color::from_rgb8(30, 210, 70)),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    assert!(incremental.incremental_render_stats().reused_compiled_plan);

    // Regression: this used to panic because the topology frame added stable batch membership
    // without adding the new leaf to `scene_command_locations`.
    scene
        .transaction()
        .replace_scene(inserted, child(Color::from_rgb8(30, 80, 220)))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);

    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    full.render_retained(&scene);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn persistent_retained_tail_layer_add_remove_patches_plan_without_ghost_draws() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_650);
    let base = RetainedNodeId::for_owner(50_651);
    let layer = RetainedNodeId::for_owner(50_652);
    let leaf = RetainedNodeId::for_owner(50_653);
    let solid = |rect: Rect, color| {
        let mut canvas = Canvas::new(64, 16, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(64, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            solid(
                Rect::new(0.0, 0.0, 64.0, 16.0),
                Color::from_rgb8(30, 70, 180),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(64, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    let initial = renderer.image();

    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(16.0, 0.0, 32.0, 16.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.5,
            },
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().pixels, initial.pixels);
    assert!(!renderer.incremental_render_stats().full_scene_sync);

    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(layer),
            None,
            leaf,
            solid(
                Rect::new(16.0, 0.0, 32.0, 16.0),
                Color::from_rgb8(240, 80, 30),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(renderer.incremental_render_stats().reused_compiled_plan);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        0
    );
    let incremental = renderer.image();
    let mut full = new_test_renderer(64, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.pixels, full.image().pixels);
    assert!(!renderer.incremental_render_stats().full_scene_sync);

    // Regression: the flat topology fast path must still install the command location used by
    // the leaf's next content update, including when the leaf belongs to an offscreen layer.
    scene
        .transaction()
        .replace_scene(
            leaf,
            solid(
                Rect::new(16.0, 0.0, 32.0, 16.0),
                Color::from_rgb8(40, 220, 120),
            ),
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    full.render_retained(&scene);
    assert_eq!(renderer.image().pixels, full.image().pixels);

    scene.transaction().remove_subtree(layer).commit().unwrap();
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().pixels, initial.pixels);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        2
    );
    assert_eq!(renderer.incremental_render_stats().chunks_rebuilt, 0);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
}

#[test]
fn persistent_retained_tail_layer_and_leaf_inserted_together_are_rendered() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_655);
    let base = RetainedNodeId::for_owner(50_656);
    let layer = RetainedNodeId::for_owner(50_657);
    let leaf = RetainedNodeId::for_owner(50_658);
    let child = |color| {
        let mut canvas = Canvas::new(32, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            child(Color::from_rgb8(30, 70, 180)),
            Affine::translate((16.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    let initial = renderer.image();

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
            leaf,
            child(Color::from_rgb8(240, 80, 30)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 1);
    assert_eq!(renderer.image().rgba8_at(8, 8), [120, 40, 15, 128]);

    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(renderer.image().pixels, full.image().pixels);

    scene.transaction().remove_subtree(layer).commit().unwrap();
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().pixels, initial.pixels);
}

#[test]
fn persistent_root_layer_insert_before_intersecting_scene_patches_plan() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_660);
    let base = RetainedNodeId::for_owner(50_661);
    let layer = RetainedNodeId::for_owner(50_662);
    let leaf = RetainedNodeId::for_owner(50_663);
    let solid = |rect: Rect, color| {
        let mut canvas = Canvas::new(64, 16, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(64, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            solid(
                Rect::new(24.0, 0.0, 64.0, 16.0),
                Color::from_rgb8(30, 70, 180),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(64, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    let initial = renderer.image();

    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            Some(base),
            layer,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(8.0, 0.0, 40.0, 16.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.5,
            },
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            leaf,
            solid(
                Rect::new(8.0, 0.0, 40.0, 16.0),
                Color::from_rgb8(240, 80, 30),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        2
    );

    let incremental = renderer.image();
    let mut full = new_test_renderer(64, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.pixels, full.image().pixels);

    scene.transaction().remove_subtree(layer).commit().unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        2
    );
    assert_eq!(renderer.image().pixels, initial.pixels);
}

#[test]
fn persistent_retained_removes_initial_middle_root_layer_without_recompiling_scene() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_664);
    let layer = RetainedNodeId::for_owner(50_665);
    let leaf = RetainedNodeId::for_owner(50_666);
    let base = RetainedNodeId::for_owner(50_667);
    let solid = |rect: Rect, color| {
        let mut canvas = Canvas::new(64, 16, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(64, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(8.0, 0.0, 40.0, 16.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.5,
            },
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            leaf,
            solid(
                Rect::new(8.0, 0.0, 40.0, 16.0),
                Color::from_rgb8(240, 80, 30),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            solid(
                Rect::new(24.0, 0.0, 64.0, 16.0),
                Color::from_rgb8(30, 70, 180),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    let mut renderer = new_test_renderer(64, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    scene.transaction().remove_subtree(layer).commit().unwrap();
    renderer.render_retained(&scene);

    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        2
    );
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    let incremental = renderer.image();
    let mut full = new_test_renderer(64, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.pixels, full.image().pixels);
}

#[test]
fn persistent_retained_inserts_layer_fragment_before_existing_layer() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_670);
    let first_layer = RetainedNodeId::for_owner(50_671);
    let first_leaf = RetainedNodeId::for_owner(50_672);
    let second_layer = RetainedNodeId::for_owner(50_673);
    let second_leaf = RetainedNodeId::for_owner(50_674);
    let layer = |opacity| RetainedLayerDescriptor::Opacity {
        path: Rect::new(0.0, 0.0, 48.0, 16.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity,
    };
    let solid = |rect: Rect, color| {
        let mut canvas = Canvas::new(64, 16, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(64, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(RetainedParent::content(root), None, first_layer, layer(0.6))
        .insert_scene(
            RetainedParent::content(first_layer),
            None,
            first_leaf,
            solid(
                Rect::new(0.0, 0.0, 32.0, 16.0),
                Color::from_rgb8(30, 70, 180),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(64, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    let initial = renderer.image();

    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            Some(first_layer),
            second_layer,
            layer(0.4),
        )
        .insert_scene(
            RetainedParent::content(second_layer),
            None,
            second_leaf,
            solid(
                Rect::new(16.0, 0.0, 48.0, 16.0),
                Color::from_rgb8(240, 80, 30),
            ),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_scene_sync);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        2
    );

    let incremental = renderer.image();
    let mut full = new_test_renderer(64, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.pixels, full.image().pixels);

    scene
        .transaction()
        .remove_subtree(second_layer)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().pixels, initial.pixels);
    assert_eq!(
        renderer.incremental_render_stats().plan_fragments_rebuilt,
        2
    );
}

#[test]
fn persistent_retained_renderers_consume_independent_cursors_and_recover_after_journal_gap() {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(50_500);
    let leaf = RetainedNodeId::for_owner(50_501);
    let child = |value| {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(value, 255 - value, value / 2),
        );
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(16, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            leaf,
            child(1),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut current = new_test_renderer(16, 16, Color::TRANSPARENT);
    let mut lagging = new_test_renderer(16, 16, Color::TRANSPARENT);
    current.render_retained(&scene);
    lagging.render_retained(&scene);

    for version in 0..=256u16 {
        scene
            .transaction()
            .replace_scene(leaf, child((version % 251) as u8 + 1))
            .commit()
            .unwrap();
        if version.is_multiple_of(64) {
            current.render_retained(&scene);
        }
    }
    lagging.render_retained(&scene);
    assert_eq!(lagging.incremental_render_stats().chunks_rebuilt, 1);
    assert!(lagging.incremental_render_stats().full_scene_sync);

    scene
        .transaction()
        .replace_scene(leaf, child(237))
        .commit()
        .unwrap();
    lagging.render_retained(&scene);
    assert_eq!(lagging.incremental_render_stats().chunks_rebuilt, 1);
    assert!(!lagging.incremental_render_stats().full_scene_sync);
    assert!(lagging.incremental_render_stats().reused_compiled_plan);

    current.render_retained(&scene);
    assert_eq!(current.image().pixels, lagging.image().pixels);
}
