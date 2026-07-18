use super::*;

#[test]
fn retained_path_removal_preserves_sparse_scan_chunk_mapping() {
    if !run_wgpu_tests() {
        return;
    }

    let leaf = |x: f64, color: Color| {
        let mut canvas = Canvas::new(64, 16, 1.0);
        canvas.push_path(
            Rect::new(x, 0.0, x + 12.0, 16.0).to_path(0.0),
            color,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
        std::rc::Rc::new(canvas)
    };
    let root = RetainedNodeId::for_owner(94_000);
    let first = RetainedNodeId::for_owner(94_001);
    let removed = RetainedNodeId::for_owner(94_002);
    let changed = RetainedNodeId::for_owner(94_003);
    let mut scene = RetainedScene::new(64, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            first,
            leaf(0.0, Color::from_rgb8(220, 40, 30)),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            removed,
            leaf(16.0, Color::from_rgb8(40, 220, 30)),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            changed,
            leaf(32.0, Color::from_rgb8(30, 40, 220)),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(64, 16, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .remove_subtree(removed)
        .replace_scene(changed, leaf(32.0, Color::from_rgb8(240, 180, 20)))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);

    let mut full = new_test_renderer(64, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn persistent_retained_scene_updates_incrementally_and_reuses_static_frames() {
    if !run_wgpu_tests() {
        return;
    }

    let child = |color| {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let root = RetainedNodeId::for_owner(50_000);
    let node = RetainedNodeId::for_owner(50_001);
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    transaction.insert_scene(
        RetainedParent::content(root),
        None,
        node,
        child(Color::from_rgb8(220, 30, 40)),
        Affine::translate((0.0, 0.0)),
    );
    transaction.commit().unwrap();

    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    let mut transaction = scene.transaction();
    transaction.replace_scene(node, child(Color::from_rgb8(30, 210, 70)));
    transaction.commit().unwrap();
    renderer.render_retained(&scene);
    assert!(!renderer.incremental_render_stats().full_redraw);
    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 1);
    assert!(renderer.incremental_render_stats().reused_compiled_plan);
    assert_eq!(renderer.incremental_render_stats().chunks_rebuilt, 1);
    assert!(renderer.incremental_render_stats().cpu_copied_bytes > 0);
    assert!(renderer.incremental_render_stats().gpu_uploaded_bytes > 0);
    assert_eq!(renderer.incremental_render_stats().tile_pages_rewritten, 0);
    assert_eq!(renderer.image().rgba8_at(8, 8), [30, 210, 70, 255]);

    renderer.render_retained(&scene);
    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 0);
    assert_eq!(renderer.incremental_render_stats().chunks_rebuilt, 0);
    assert_eq!(renderer.incremental_render_stats().gpu_uploaded_bytes, 0);
    assert_eq!(
        renderer
            .incremental_render_stats()
            .retained_surface_nodes_scanned,
        0,
        "an unchanged frame must not rescan every node for surface ownership"
    );
    assert!(
        renderer
            .incremental_render_stats()
            .materialized_scene_reused
    );

    let mut transaction = scene.transaction();
    transaction.invalidate_rect(Rect::new(0.0, 0.0, 8.0, 8.0));
    transaction.commit().unwrap();
    renderer.start_profile();
    renderer.render_retained(&scene);
    let profile = renderer.end_profile().clone();
    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 1);
    assert_eq!(renderer.incremental_render_stats().chunks_rebuilt, 0);
    assert_eq!(renderer.incremental_render_stats().gpu_uploaded_bytes, 0);
    assert!(
        renderer
            .incremental_render_stats()
            .materialized_scene_reused
    );
    assert!(
        profile
            .entries()
            .iter()
            .all(|entry| entry.name != "prepare"),
        "raster-only invalidation must reuse prepared scene buffers"
    );
    assert!(
        profile
            .entries()
            .iter()
            .all(|entry| entry.name != "retained.damage.propagate"),
        "manual invalidation in a dependency-free scene must not walk every retained command"
    );

    scene
        .transaction()
        .replace_scene(node, child(Color::from_rgb8(20, 80, 230)))
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(8, 8), [20, 80, 230, 255]);
}

#[test]
fn persistent_affine_path_matches_immediate_geometry_and_updates_damage() {
    if !run_wgpu_tests() {
        return;
    }

    let shape = Rect::new(4.0, 6.0, 28.0, 18.0).to_path(0.1);
    let color = Color::from_rgb8(35, 145, 230);
    let mut leaf = Canvas::new(32, 24, 1.0);
    leaf.push_path(
        shape.clone(),
        color,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    let root = RetainedNodeId::for_owner(50_090);
    let node = RetainedNodeId::for_owner(50_091);
    let first = Affine::translate((40.0, 24.0))
        * Affine::rotate(0.4)
        * Affine::scale_non_uniform(1.25, 0.8);
    let mut scene = RetainedScene::new(96, 80, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            node,
            std::rc::Rc::new(leaf),
            first,
        )
        .commit()
        .unwrap();

    let render_reference = |transform| {
        let mut canvas = Canvas::new(96, 80, 1.0);
        canvas.push_path(shape.clone(), color, transform, FillRule::NonZero, 0.1);
        let mut renderer = new_test_renderer(96, 80, Color::TRANSPARENT);
        renderer.render(&canvas);
        renderer.image().pixels.clone()
    };
    let mut retained = new_test_renderer(96, 80, Color::TRANSPARENT);
    retained.render_retained(&scene);
    assert_eq!(retained.image().pixels, render_reference(first));

    let second = Affine::translate((68.0, 46.0))
        * Affine::rotate(-0.55)
        * Affine::scale_non_uniform(0.75, 1.4);
    scene
        .transaction()
        .set_transform(node, second)
        .commit()
        .unwrap();
    retained.render_retained(&scene);
    assert_eq!(retained.image().pixels, render_reference(second));
    assert!(!retained.incremental_render_stats().full_redraw);
    assert_eq!(retained.incremental_render_stats().chunks_rebuilt, 1);
}

#[test]
fn persistent_bounded_translation_uses_fixed_damage_tiles() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_094);
    let node = RetainedNodeId::for_owner(50_095);
    let mut leaf = Canvas::new(64, 32, 1.0);
    leaf.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let damage = Rect::new(0.0, 0.0, 48.0, 16.0);
    let mut scene = RetainedScene::new(64, 32, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_bounded_scene(
            RetainedParent::content(root),
            None,
            node,
            std::rc::Rc::new(leaf),
            Affine::IDENTITY,
            damage,
        )
        .commit()
        .unwrap();

    let mut renderer = new_test_renderer(64, 32, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    scene
        .transaction()
        .set_bounded_translation(node, Affine::translate((8.0, 0.0)), damage)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);

    let mut expected = Canvas::new(64, 32, 1.0);
    expected.push_rect(
        Rect::new(8.0, 0.0, 16.0, 8.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let mut reference = new_test_renderer(64, 32, Color::TRANSPARENT);
    reference.render(&expected);
    assert_eq!(renderer.image().pixels, reference.image().pixels);

    let stats = renderer.incremental_render_stats();
    assert!(!stats.full_redraw);
    assert_eq!(stats.changed_tiles, 3);
    assert_eq!(stats.dirty_tiles, 3);
}

#[test]
fn persistent_bounded_translation_clears_the_previous_domain_when_bounds_change() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_096);
    let node = RetainedNodeId::for_owner(50_097);
    let mut leaf = Canvas::new(64, 16, 1.0);
    leaf.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let old_damage = Rect::new(0.0, 0.0, 16.0, 16.0);
    let new_damage = Rect::new(32.0, 0.0, 48.0, 16.0);
    let mut scene = RetainedScene::new(64, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_bounded_scene(
            RetainedParent::content(root),
            None,
            node,
            std::rc::Rc::new(leaf),
            Affine::IDENTITY,
            old_damage,
        )
        .commit()
        .unwrap();

    let mut renderer = new_test_renderer(64, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    scene
        .transaction()
        .set_bounded_translation(node, Affine::translate((32.0, 0.0)), new_damage)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);

    let mut expected = Canvas::new(64, 16, 1.0);
    expected.push_rect(
        Rect::new(32.0, 0.0, 40.0, 8.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let mut reference = new_test_renderer(64, 16, Color::TRANSPARENT);
    reference.render(&expected);
    assert_eq!(renderer.image().pixels, reference.image().pixels);
    assert_eq!(renderer.incremental_render_stats().changed_tiles, 2);
}

#[test]
fn persistent_node_affine_composes_with_canvas_draw_affine() {
    if !run_wgpu_tests() {
        return;
    }

    let shape = Rect::new(2.0, 3.0, 18.0, 11.0).to_path(0.1);
    let color = Color::from_rgb8(180, 65, 225);
    let draw_transform = Affine::translate((7.0, 5.0)) * Affine::rotate(0.3);
    let node_transform = Affine::translate((42.0, 26.0))
        * Affine::scale_non_uniform(1.2, 0.75)
        * Affine::rotate(-0.2);
    let mut leaf = Canvas::new(32, 24, 1.0);
    leaf.push_path(shape.clone(), color, draw_transform, FillRule::NonZero, 0.1);
    let root = RetainedNodeId::for_owner(50_096);
    let node = RetainedNodeId::for_owner(50_097);
    let mut scene = RetainedScene::new(96, 72, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            node,
            std::rc::Rc::new(leaf),
            node_transform,
        )
        .commit()
        .unwrap();

    let mut expected = Canvas::new(96, 72, 1.0);
    expected.push_path(
        shape,
        color,
        node_transform * draw_transform,
        FillRule::NonZero,
        0.1,
    );
    let mut expected_renderer = new_test_renderer(96, 72, Color::TRANSPARENT);
    expected_renderer.render(&expected);
    let mut actual_renderer = new_test_renderer(96, 72, Color::TRANSPARENT);
    actual_renderer.render_retained(&scene);
    assert_eq!(
        actual_renderer.image().pixels,
        expected_renderer.image().pixels
    );
}

#[test]
fn persistent_rotated_sdf_rect_does_not_fill_its_axis_aligned_bounds() {
    if !run_wgpu_tests() {
        return;
    }

    let mut leaf = Canvas::new(20, 20, 1.0);
    let color = Color::from_rgb8(230, 70, 35);
    leaf.push_rect(Rect::new(0.0, 0.0, 20.0, 20.0), crate::Radius::ZERO, color);
    let root = RetainedNodeId::for_owner(50_092);
    let node = RetainedNodeId::for_owner(50_093);
    let transform = Affine::translate((40.0, 20.0))
        * Affine::rotate(std::f64::consts::FRAC_PI_4)
        * Affine::translate((-10.0, 0.0));
    let mut scene = RetainedScene::new(80, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            node,
            std::rc::Rc::new(leaf),
            transform,
        )
        .commit()
        .unwrap();

    let mut renderer = new_test_renderer(80, 64, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(33, 27), [230, 70, 35, 255]);
    assert_eq!(renderer.image().rgba8_at(20, 14), [0, 0, 0, 0]);
}

#[test]
fn persistent_translated_sdf_rect_uses_world_coordinates() {
    if !run_wgpu_tests() {
        return;
    }
    let mut leaf = Canvas::new(16, 16, 1.0);
    leaf.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 210, 70),
    );
    let root = RetainedNodeId::for_owner(50_094);
    let node = RetainedNodeId::for_owner(50_095);
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            node,
            std::rc::Rc::new(leaf),
            Affine::translate((8.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(4, 8), [0, 0, 0, 0]);
    assert_eq!(renderer.image().rgba8_at(10, 8), [30, 210, 70, 255]);
}

#[test]
fn persistent_surface_resize_matches_force_full_without_rebuilding_chunks() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_100);
    let node = RetainedNodeId::for_owner(50_101);
    let mut leaf = Canvas::new(96, 32, 1.0);
    leaf.push_path(
        Rect::new(8.0, 4.0, 88.0, 28.0).to_path(0.1),
        Color::from_rgb8(40, 120, 230),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    let mut scene = RetainedScene::new(32, 32, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            node,
            std::rc::Rc::new(leaf),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut incremental = new_test_renderer(32, 32, Color::TRANSPARENT);
    incremental.render_retained(&scene);

    for width in [96, 24, 72] {
        scene.transaction().resize(width, 32, 1.0).commit().unwrap();
        incremental.render_retained(&scene);
        assert_eq!(incremental.incremental_render_stats().chunks_rebuilt, 0);
        assert!(!incremental.incremental_render_stats().full_scene_sync);

        let mut full = new_test_renderer(width, 32, Color::TRANSPARENT);
        let mut config = full.incremental_render_config();
        config.mode = crate::IncrementalRenderMode::ForceFull;
        full.set_incremental_render_config(config);
        full.render(&scene.to_canvas());
        assert_eq!(incremental.image().pixels, full.image().pixels);
    }

    scene
        .transaction()
        .set_transform(node, Affine::translate((2.0, 0.0)))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    let mut full = new_test_renderer(72, 32, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&scene.to_canvas());
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn persistent_move_preserves_backdrop_children_and_shadow() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_003);
    let background_id = RetainedNodeId::for_owner(50_004);
    let card_id = RetainedNodeId::for_owner(50_005);
    let mut background = Canvas::new(96, 48, 1.0);
    background.push_rect(
        Rect::new(0.0, 0.0, 96.0, 48.0),
        crate::Radius::ZERO,
        Color::from_rgb8(35, 90, 180),
    );
    let mut card = Canvas::new(32, 24, 1.0);
    let card_bounds = Rect::new(0.0, 0.0, 32.0, 24.0);
    card.push_rect_shadow(
        card_bounds,
        crate::Radius::all(6.0),
        crate::RectShadowOptions::new(0.0, 3.0, 3.0, 0.8),
        Color::BLACK,
    );
    card.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 3,
            tint: Color::from_rgba8(255, 255, 255, 32),
            ..Default::default()
        }),
        Region::rect(card_bounds, crate::Radius::all(6.0)),
    );
    card.push_rect(
        card_bounds,
        crate::Radius::all(6.0),
        Color::from_rgba8(255, 255, 255, 48),
    );
    card.pop_layer();

    let mut scene = RetainedScene::new(96, 48, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background_id,
            std::rc::Rc::new(background),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            card_id,
            std::rc::Rc::new(card),
            Affine::translate((8.0, 8.0)),
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(96, 48, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .set_transform(card_id, Affine::translate((48.0, 8.0)))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    let stats = incremental.incremental_render_stats();
    assert_eq!(stats.chunks_rebuilt, 1);
    assert_eq!(stats.plan_fragments_rebuilt, 1);
    assert!(stats.reused_compiled_plan);
    assert!(!stats.full_scene_sync);

    let mut full = new_test_renderer(96, 48, Color::TRANSPARENT);
    full.render_retained(&scene);
    let mismatches = incremental
        .image()
        .pixels
        .iter()
        .zip(&full.image().pixels)
        .filter(|(actual, expected)| actual != expected)
        .count();
    assert_eq!(mismatches, 0);
}

#[test]
fn persistent_paint_replacement_preserves_layered_draw_batch_membership() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_006);
    let background_id = RetainedNodeId::for_owner(50_007);
    let card_id = RetainedNodeId::for_owner(50_008);
    let mut background = Canvas::new(96, 48, 1.0);
    background.push_rect(
        Rect::new(0.0, 0.0, 96.0, 48.0),
        crate::Radius::ZERO,
        Color::from_rgb8(35, 90, 180),
    );
    let card = |tint, border| {
        let bounds = Rect::new(0.0, 0.0, 32.0, 24.0);
        let mut canvas = Canvas::new(32, 32, 1.0);
        canvas.push_rect_shadow(
            bounds,
            crate::Radius::all(6.0),
            crate::RectShadowOptions::new(0.0, 3.0, 3.0, 0.8),
            Color::BLACK,
        );
        canvas.push_backdrop_layer(
            Filter::RectLiquidGlass(RectLiquidGlass {
                blur_radius: 3,
                tint: Color::from_rgba8(255, 255, 255, 32),
                ..Default::default()
            }),
            Region::rect(bounds, crate::Radius::all(6.0)),
        );
        canvas.push_rect(bounds, crate::Radius::all(6.0), tint);
        canvas.push_rect_stroke_widths(
            bounds,
            crate::Radius::all(6.0),
            crate::StrokeWidths::all(1.0),
            border,
        );
        canvas.pop_layer();
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(96, 48, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background_id,
            std::rc::Rc::new(background),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            card_id,
            card(
                Color::from_rgba8(255, 255, 255, 24),
                Color::from_rgba8(255, 255, 255, 96),
            ),
            Affine::translate((32.0, 8.0)),
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(96, 48, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .replace_scene(
            card_id,
            card(
                Color::from_rgba8(255, 255, 255, 40),
                Color::from_rgba8(255, 255, 255, 220),
            ),
        )
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    assert!(incremental.incremental_render_stats().reused_compiled_plan);

    let mut full = new_test_renderer(96, 48, Color::TRANSPARENT);
    full.render_retained(&scene);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn persistent_filter_manual_invalidation_skips_command_tree_propagation() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(54_000);
    let layer = RetainedNodeId::for_owner(54_001);
    let leaf = RetainedNodeId::for_owner(54_002);
    let mut child = Canvas::new(16, 16, 1.0);
    child.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 130, 220),
    );
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            RetainedLayerDescriptor::Filter {
                filter: Filter::Opacity(0.75),
                sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), crate::Radius::ZERO),
            },
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            leaf,
            std::rc::Rc::new(child),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    let mut renderer = new_test_renderer(64, 64, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    assert_eq!(renderer.pending_local_scene_resources.len(), 1);
    let pooled_config = renderer.pending_local_scene_resources[0]
        .config
        .binding_key();
    scene
        .transaction()
        .invalidate_rect(Rect::new(48.0, 48.0, 56.0, 56.0))
        .commit()
        .unwrap();
    renderer.start_profile();
    renderer.render_retained(&scene);
    let profile = renderer.end_profile().clone();
    assert_eq!(
        renderer
            .incremental_render_stats()
            .rerendered_offscreen_surfaces,
        0
    );
    assert_profile_missing(&profile, "retained.damage.propagate");

    scene
        .transaction()
        .invalidate_rect(Rect::new(0.0, 0.0, 8.0, 8.0))
        .commit()
        .unwrap();
    renderer.start_profile();
    renderer.render_retained(&scene);
    let profile = renderer.end_profile().clone();
    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 1);
    assert_profile_missing(&profile, "retained.damage.propagate");
    assert_eq!(renderer.pending_local_scene_resources.len(), 1);
    assert_eq!(
        renderer.pending_local_scene_resources[0]
            .config
            .binding_key(),
        pooled_config,
        "cropped filter updates must reuse their scene-bound GPU allocation set"
    );
}

#[test]
fn persistent_backdrop_manual_invalidation_uses_indexed_dependency_damage() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(55_000);
    let background = RetainedNodeId::for_owner(55_001);
    let backdrop = RetainedNodeId::for_owner(55_002);
    let mut child = Canvas::new(64, 64, 1.0);
    child.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 130, 220),
    );
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background,
            std::rc::Rc::new(child),
            Affine::translate((0.0, 0.0)),
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Opacity(0.75),
                sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), crate::Radius::ZERO),
            },
        )
        .commit()
        .unwrap();

    let mut renderer = new_test_renderer(64, 64, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    scene
        .transaction()
        .invalidate_rect(Rect::new(48.0, 48.0, 56.0, 56.0))
        .commit()
        .unwrap();
    renderer.start_profile();
    renderer.render_retained(&scene);
    let profile = renderer.end_profile().clone();
    assert_eq!(
        renderer
            .incremental_render_stats()
            .rerendered_offscreen_surfaces,
        0
    );
    assert_profile_missing(&profile, "retained.damage.propagate");

    scene
        .transaction()
        .invalidate_rect(Rect::new(0.0, 0.0, 8.0, 8.0))
        .commit()
        .unwrap();
    renderer.start_profile();
    renderer.render_retained(&scene);
    let profile = renderer.end_profile().clone();
    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 1);
    assert_eq!(
        renderer
            .incremental_render_stats()
            .rerendered_offscreen_surfaces,
        1
    );
    assert_profile_missing(&profile, "retained.damage.propagate");
}

#[test]
fn persistent_many_layers_execute_only_batches_touching_damage() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_010);
    let mut leaf = Canvas::new(16, 16, 1.0);
    leaf.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let leaf = std::rc::Rc::new(leaf);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    for index in 0..16 {
        let x = (index % 4) as f64 * 16.0;
        let y = (index / 4) as f64 * 16.0;
        let layer = RetainedNodeId::for_owner(50_020 + index);
        transaction
            .insert_layer(
                RetainedParent::content(root),
                None,
                layer,
                RetainedLayerDescriptor::Opacity {
                    path: Rect::new(x, y, x + 16.0, y + 16.0).to_path(0.1),
                    transform: Affine::IDENTITY,
                    tolerance: 0.1,
                    opacity: 0.5,
                },
            )
            .insert_scene(
                RetainedParent::content(layer),
                None,
                RetainedNodeId::for_owner(50_100 + index),
                leaf.clone(),
                Affine::translate((x, y)),
            );
    }
    transaction.commit().unwrap();

    let mut renderer = new_test_renderer(64, 64, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    scene
        .transaction()
        .update_layer(
            RetainedNodeId::for_owner(50_025),
            RetainedLayerDescriptor::Opacity {
                path: Rect::new(16.0, 16.0, 32.0, 32.0).to_path(0.1),
                transform: Affine::IDENTITY,
                tolerance: 0.1,
                opacity: 0.25,
            },
        )
        .commit()
        .unwrap();
    renderer.render_retained(&scene);

    assert!(!renderer.incremental_render_stats().full_redraw);
    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 1);
    assert_eq!(renderer.incremental_render_stats().root_draw_batches, 1);
    let reference = scene.to_canvas();
    let mut full = new_test_renderer(64, 64, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&reference);
    assert_eq!(renderer.image().pixels, full.image().pixels);
}

#[test]
fn persistent_retained_scene_rebuilds_plan_when_local_commands_change() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_010);
    let node = RetainedNodeId::for_owner(50_011);
    let plain = {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(220, 30, 40),
        );
        std::rc::Rc::new(canvas)
    };
    let layered = {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_opacity_layer(
            Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            0.0,
            0.5,
        );
        canvas.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(30, 210, 70),
        );
        canvas.pop_layer();
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(16, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            node,
            plain,
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(16, 16, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .replace_scene(node, layered)
        .commit()
        .unwrap();
    incremental.render_retained(&scene);

    let mut full = new_test_renderer(16, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    assert!((126..=129).contains(&incremental.image().rgba8_at(8, 8)[3]));
    assert!(!incremental.incremental_render_stats().reused_compiled_plan);
}

#[test]
fn persistent_retained_scene_preserves_order_after_variable_length_reallocation() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_100);
    let back = RetainedNodeId::for_owner(50_101);
    let front = RetainedNodeId::for_owner(50_102);
    let solid = |color| {
        let mut canvas = Canvas::new(32, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    transaction
        .insert_scene(
            RetainedParent::content(root),
            None,
            back,
            solid(Color::from_rgb8(220, 30, 40)),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            front,
            solid(Color::from_rgb8(30, 60, 220)),
            Affine::translate((0.0, 0.0)),
        );
    transaction.commit().unwrap();

    let mut incremental = new_test_renderer(32, 16, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    let mut longer = Canvas::new(32, 16, 1.0);
    longer.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 210, 70),
    );
    longer.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(240, 210, 40),
    );
    let mut transaction = scene.transaction();
    transaction.replace_scene(back, std::rc::Rc::new(longer));
    transaction.commit().unwrap();
    incremental.render_retained(&scene);
    assert_eq!(incremental.image().rgba8_at(8, 8), [30, 60, 220, 255]);

    let mut transaction = scene.transaction();
    transaction.move_before(front, back);
    transaction.commit().unwrap();
    incremental.render_retained(&scene);

    let reference = scene.to_canvas();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&reference);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    assert_eq!(incremental.image().rgba8_at(8, 8), [30, 210, 70, 255]);
    assert_eq!(incremental.image().rgba8_at(24, 8), [240, 210, 40, 255]);
}

#[test]
fn persistent_variable_length_update_uploads_only_changed_allocations() {
    if !run_wgpu_tests() {
        return;
    }

    let one_draw = {
        let mut canvas = Canvas::new(8, 8, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 8.0, 8.0),
            crate::Radius::ZERO,
            Color::from_rgb8(30, 130, 220),
        );
        std::rc::Rc::new(canvas)
    };
    let two_draws = {
        let mut canvas = Canvas::new(8, 8, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 4.0, 8.0),
            crate::Radius::ZERO,
            Color::from_rgb8(30, 210, 70),
        );
        canvas.push_rect(
            Rect::new(4.0, 0.0, 8.0, 8.0),
            crate::Radius::ZERO,
            Color::from_rgb8(240, 210, 40),
        );
        std::rc::Rc::new(canvas)
    };
    let root = RetainedNodeId::for_owner(50_200);
    let changed = RetainedNodeId::for_owner(50_201);
    let mut scene = RetainedScene::new(256, 128, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    for index in 0..512 {
        transaction.insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(50_201 + index),
            one_draw.clone(),
            Affine::translate(((index % 32) as f64 * 8.0, (index / 32) as f64 * 8.0)),
        );
    }
    transaction.commit().unwrap();

    let mut renderer = new_test_renderer(256, 128, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    scene
        .transaction()
        .replace_scene(changed, two_draws.clone())
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(
        renderer.incremental_render_stats().gpu_uploaded_bytes < 16 * 1024,
        "one variable-length chunk must not relocate and upload the full paint arena: {:?}",
        renderer.incremental_render_stats()
    );
    assert_eq!(renderer.image().rgba8_at(2, 4), [30, 210, 70, 255]);
    assert_eq!(renderer.image().rgba8_at(6, 4), [240, 210, 40, 255]);

    scene
        .transaction()
        .replace_scene(changed, one_draw)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert!(renderer.incremental_render_stats().gpu_uploaded_bytes < 16 * 1024);
}

#[test]
fn persistent_resource_variable_length_update_is_local_and_matches_full_render() {
    if !run_wgpu_tests() {
        return;
    }

    let image = std::rc::Rc::new(Image::from_rgba8(
        2,
        2,
        [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
    ));
    let child = |two_draws| {
        let mut canvas = Canvas::new(8, 8, 1.0);
        let end = if two_draws { 4.0 } else { 8.0 };
        canvas
            .push_image(
                Rect::new(0.0, 0.0, end, 8.0),
                image.clone(),
                Extend::Pad,
                PatternSampling::Bilinear,
            )
            .unwrap();
        if two_draws {
            canvas
                .push_image(
                    Rect::new(4.0, 0.0, 8.0, 8.0),
                    image.clone(),
                    Extend::Pad,
                    PatternSampling::Bilinear,
                )
                .unwrap();
        }
        std::rc::Rc::new(canvas)
    };
    let one_draw = child(false);
    let two_draws = child(true);
    let root = RetainedNodeId::for_owner(51_000);
    let changed = RetainedNodeId::for_owner(51_001);
    let mut scene = RetainedScene::new(256, 128, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    for index in 0..512 {
        transaction.insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(51_001 + index),
            one_draw.clone(),
            Affine::translate(((index % 32) as f64 * 8.0, (index / 32) as f64 * 8.0)),
        );
    }
    transaction.commit().unwrap();

    let mut incremental = new_test_renderer(256, 128, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .replace_scene(changed, two_draws)
        .commit()
        .unwrap();
    incremental.render_retained(&scene);

    let mut full = new_test_renderer(256, 128, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&scene.to_canvas());
    assert_eq!(incremental.image().pixels, full.image().pixels);
    assert!(
        incremental.incremental_render_stats().gpu_uploaded_bytes < 16 * 1024,
        "one resource-pattern chunk must not repatch and upload every brush: {:?}",
        incremental.incremental_render_stats()
    );
}

#[test]
fn persistent_resource_brush_repatches_after_atlas_placement_changes() {
    if !run_wgpu_tests() {
        return;
    }

    let sampled = ImageKey::new(20);
    let mut child = Canvas::new(8, 8, 1.0);
    child
        .push_image_key(
            Rect::new(0.0, 0.0, 8.0, 8.0),
            sampled,
            Extend::Pad,
            PatternSampling::Nearest,
        )
        .unwrap();
    let root = RetainedNodeId::for_owner(52_000);
    let mut scene = RetainedScene::new(8, 8, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(52_001),
            std::rc::Rc::new(child),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    let mut renderer = new_test_renderer(8, 8, Color::TRANSPARENT);
    assert!(renderer.insert_image(sampled, Image::from_rgba8(1, 1, [20, 210, 70, 255])));
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(4, 4), [20, 210, 70, 255]);

    // A lower key sorts ahead of the sampled image and moves its atlas rectangle. The scene and
    // draw allocation remain unchanged, so only the image-resource generation can trigger the
    // required brush placement repatch.
    assert!(renderer.insert_image(
        ImageKey::new(1),
        Image::from_rgba8(1, 1, [240, 30, 40, 255])
    ));
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(4, 4), [20, 210, 70, 255]);
}

#[test]
fn persistent_resource_brush_membership_tracks_incremental_replacement() {
    if !run_wgpu_tests() {
        return;
    }

    let solid = {
        let mut canvas = Canvas::new(8, 8, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 8.0, 8.0),
            crate::Radius::ZERO,
            Color::from_rgb8(30, 80, 220),
        );
        std::rc::Rc::new(canvas)
    };
    let key = ImageKey::new(30);
    let resource = {
        let mut canvas = Canvas::new(8, 8, 1.0);
        canvas
            .push_image_key(
                Rect::new(0.0, 0.0, 8.0, 8.0),
                key,
                Extend::Pad,
                PatternSampling::Nearest,
            )
            .unwrap();
        std::rc::Rc::new(canvas)
    };
    let root = RetainedNodeId::for_owner(53_000);
    let leaf = RetainedNodeId::for_owner(53_001);
    let mut scene = RetainedScene::new(8, 8, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            leaf,
            solid.clone(),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(8, 8, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(4, 4), [30, 80, 220, 255]);

    scene
        .transaction()
        .replace_scene(leaf, resource)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(4, 4), [0, 0, 0, 0]);

    // Mutations made while the resource table is empty are picked up by the one-time membership
    // rebuild when the first image arrives.
    assert!(renderer.insert_image(key, Image::from_rgba8(1, 1, [20, 210, 70, 255])));
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(4, 4), [20, 210, 70, 255]);

    scene
        .transaction()
        .replace_scene(leaf, solid)
        .commit()
        .unwrap();
    renderer.render_retained(&scene);
    assert_eq!(renderer.image().rgba8_at(4, 4), [30, 80, 220, 255]);
}

#[test]
fn persistent_retained_scene_recollects_layer_influence_after_leaf_change() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_200);
    let layer = RetainedNodeId::for_owner(50_201);
    let leaf = RetainedNodeId::for_owner(50_202);
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
            leaf,
            child(Color::from_rgb8(220, 30, 40)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(32, 16, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .replace_scene(leaf, child(Color::from_rgb8(30, 210, 70)))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);

    let mut longer = Canvas::new(32, 16, 1.0);
    longer.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 210, 70),
    );
    longer.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(240, 210, 40),
    );
    scene
        .transaction()
        .replace_scene(leaf, std::rc::Rc::new(longer))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);

    let mut same_shape = Canvas::new(32, 16, 1.0);
    same_shape.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(50, 190, 90),
    );
    same_shape.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(220, 180, 30),
    );
    scene
        .transaction()
        .replace_scene(leaf, std::rc::Rc::new(same_shape))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    assert!(incremental.incremental_render_stats().reused_compiled_plan);

    let reference = scene.to_canvas();
    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&reference);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn persistent_retained_layer_descriptor_update_patches_only_the_layer_chunk() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_210);
    let layer = RetainedNodeId::for_owner(50_211);
    let leaf = RetainedNodeId::for_owner(50_212);
    let mut child = Canvas::new(32, 16, 1.0);
    child.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(40, 160, 220),
    );
    let opacity = |width, opacity| RetainedLayerDescriptor::Opacity {
        path: Rect::new(0.0, 0.0, width, 16.0).to_path(0.1),
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
            layer,
            opacity(32.0, 0.75),
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            leaf,
            std::rc::Rc::new(child),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(32, 16, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .update_layer(layer, opacity(32.0, 0.25))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);

    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    assert_eq!(incremental.incremental_render_stats().chunks_rebuilt, 1);
    assert_eq!(
        incremental
            .incremental_render_stats()
            .plan_fragments_rebuilt,
        1
    );
    assert!(incremental.incremental_render_stats().reused_compiled_plan);
    assert!((62..=65).contains(&incremental.image().rgba8_at(8, 8)[3]));

    scene
        .transaction()
        .update_layer(layer, opacity(16.0, 0.25))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    assert_eq!(incremental.image().rgba8_at(24, 8), [0, 0, 0, 0]);
    scene
        .transaction()
        .update_layer(layer, opacity(32.0, 0.25))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    assert!((62..=65).contains(&incremental.image().rgba8_at(24, 8)[3]));
}

#[test]
fn persistent_retained_filter_and_mask_updates_patch_offscreen_plan_fragments() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_220);
    let filter = RetainedNodeId::for_owner(50_221);
    let filter_leaf = RetainedNodeId::for_owner(50_222);
    let mask = RetainedNodeId::for_owner(50_223);
    let content_leaf = RetainedNodeId::for_owner(50_224);
    let mask_leaf = RetainedNodeId::for_owner(50_225);
    let region = Region::rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO);
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
                filter: Filter::Opacity(0.75),
                sample_region: region.clone(),
            },
        )
        .insert_scene(
            RetainedParent::content(filter),
            None,
            filter_leaf,
            solid(Color::from_rgb8(40, 120, 230)),
            Affine::translate((0.0, 0.0)),
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            mask,
            RetainedLayerDescriptor::Mask(Mask {
                region: region.clone(),
                kind: MaskKind::Alpha,
            }),
        )
        .insert_scene(
            RetainedParent::content(mask),
            None,
            content_leaf,
            solid(Color::from_rgb8(230, 80, 30)),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::mask(mask),
            None,
            mask_leaf,
            solid(Color::from_rgb8(40, 220, 60)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(32, 16, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .update_layer(
            filter,
            RetainedLayerDescriptor::Filter {
                filter: Filter::Opacity(0.25),
                sample_region: region.clone(),
            },
        )
        .update_layer(
            mask,
            RetainedLayerDescriptor::Mask(Mask {
                region,
                kind: MaskKind::Luminance,
            }),
        )
        .commit()
        .unwrap();
    incremental.render_retained(&scene);

    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    assert_eq!(incremental.incremental_render_stats().chunks_rebuilt, 2);
    assert!(incremental.incremental_render_stats().reused_compiled_plan);
}

#[test]
fn persistent_retained_backdrop_background_revision_matches_force_full() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_226);
    let background = RetainedNodeId::for_owner(50_227);
    let backdrop = RetainedNodeId::for_owner(50_228);
    let foreground = RetainedNodeId::for_owner(50_229);
    let chained_backdrop = RetainedNodeId::for_owner(502_261);
    let chained_foreground = RetainedNodeId::for_owner(502_262);
    let region = Region::rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO);
    let solid = |color| {
        let mut canvas = Canvas::new(32, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background,
            solid(Color::from_rgb8(30, 70, 210)),
            Affine::translate((0.0, 0.0)),
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Opacity(0.5),
                sample_region: region,
            },
        )
        .insert_scene(
            RetainedParent::content(backdrop),
            None,
            foreground,
            solid(Color::from_rgba8(230, 70, 30, 128)),
            Affine::translate((0.0, 0.0)),
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            chained_backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Opacity(0.75),
                sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 16.0), crate::Radius::ZERO),
            },
        )
        .insert_scene(
            RetainedParent::content(chained_backdrop),
            None,
            chained_foreground,
            solid(Color::from_rgba8(40, 80, 230, 96)),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(32, 16, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .replace_scene(background, solid(Color::from_rgb8(30, 210, 80)))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);

    let mut full = new_test_renderer(32, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    assert!(
        incremental
            .incremental_render_stats()
            .rerendered_offscreen_surfaces
            > 0
    );
}

#[test]
fn persistent_scene_embedded_backdrop_tracks_earlier_moving_scene() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_263);
    let background = RetainedNodeId::for_owner(50_264);
    let card = RetainedNodeId::for_owner(50_265);
    let panel = RetainedNodeId::for_owner(50_266);
    let solid = |width, height, color| {
        let mut canvas = Canvas::new(width, height, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
            crate::Radius::ZERO,
            color,
        );
        std::rc::Rc::new(canvas)
    };
    let mut panel_canvas = Canvas::new(40, 32, 1.0);
    let panel_rect = Rect::new(0.0, 0.0, 40.0, 32.0);
    panel_canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 3,
            tint: Color::from_rgba8(255, 255, 255, 36),
            refraction_thickness: 10.0,
            refraction_factor: 1.5,
            ..RectLiquidGlass::default()
        }),
        Region::rect(panel_rect, crate::Radius::all(6.0)),
    );
    panel_canvas.pop_layer();

    let mut scene = RetainedScene::new(96, 48, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background,
            solid(96, 48, Color::from_rgb8(28, 48, 76)),
            Affine::translate((0.0, 0.0)),
        )
        // The card is painted before the panel, so the later backdrop must sample it.
        .insert_scene(
            RetainedParent::content(root),
            None,
            card,
            solid(16, 16, Color::from_rgb8(235, 48, 38)),
            Affine::translate((8.0, 16.0)),
        )
        // This is deliberately a Scene node containing an ordinary backdrop command. Cached UI
        // widgets use this shape instead of a RetainedLayerDescriptor::Backdrop hierarchy node.
        .insert_scene(
            RetainedParent::content(root),
            None,
            panel,
            std::rc::Rc::new(panel_canvas),
            Affine::translate((48.0, 8.0)),
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(96, 48, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .set_transform(card, Affine::translate((56.0, 16.0)))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);

    let mut full = new_test_renderer(96, 48, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_retained(&scene);
    let incremental_image = incremental.image();
    let full_image = full.image();
    let difference = incremental_image
        .pixels
        .iter()
        .zip(&full_image.pixels)
        .position(|(actual, expected)| actual != expected);
    assert_eq!(
        difference, None,
        "embedded backdrop retained output diverged"
    );
    assert!(
        incremental
            .incremental_render_stats()
            .rerendered_offscreen_surfaces
            > 0,
        "moving an earlier scene into an embedded backdrop must invalidate its retained surface"
    );
}

#[test]
fn persistent_embedded_liquid_glass_updates_only_damaged_surface_tiles() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_267);
    let background = RetainedNodeId::for_owner(50_268);
    let card = RetainedNodeId::for_owner(50_269);
    let panel = RetainedNodeId::for_owner(50_270);
    let mut background_canvas = Canvas::new(512, 384, 1.0);
    for y in (0..384).step_by(32) {
        for x in (0..512).step_by(32) {
            background_canvas.push_rect(
                Rect::new(
                    f64::from(x),
                    f64::from(y),
                    f64::from(x + 32),
                    f64::from(y + 32),
                ),
                crate::Radius::ZERO,
                if (x / 32 + y / 32) % 2 == 0 {
                    Color::from_rgb8(34, 92, 156)
                } else {
                    Color::from_rgb8(166, 58, 108)
                },
            );
        }
    }
    let mut card_canvas = Canvas::new(24, 24, 1.0);
    card_canvas.push_rect(
        Rect::new(0.0, 0.0, 24.0, 24.0),
        crate::Radius::all(5.0),
        Color::from_rgb8(245, 214, 42),
    );
    let mut panel_canvas = Canvas::new(320, 256, 1.0);
    let panel_bounds = Rect::new(0.0, 0.0, 320.0, 256.0);
    panel_canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 5,
            blur_sampling: BlurSampling::FULL_RES,
            tint: Color::from_rgba8(255, 255, 255, 20),
            refraction_thickness: 24.0,
            refraction_factor: 2.0,
            refraction_dispersion: 8.0,
            ..RectLiquidGlass::default()
        }),
        Region::rect(panel_bounds, crate::Radius::all(20.0)),
    );
    panel_canvas.push_rect(
        panel_bounds,
        crate::Radius::all(20.0),
        Color::from_rgba8(10, 18, 30, 40),
    );
    panel_canvas.pop_layer();

    let mut scene = RetainedScene::new(512, 384, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background,
            std::rc::Rc::new(background_canvas),
            Affine::translate((0.0, 0.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            card,
            std::rc::Rc::new(card_canvas),
            Affine::translate((128.0, 176.0)),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            panel,
            std::rc::Rc::new(panel_canvas),
            Affine::translate((96.0, 64.0)),
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(512, 384, Color::TRANSPARENT);
    let mut incremental_config = incremental.incremental_render_config();
    incremental_config.capture_active_tiles = true;
    incremental.set_incremental_render_config(incremental_config);
    incremental.render_retained(&scene);
    let mut full = new_test_renderer(512, 384, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);

    for x in [160.0, 208.0, 256.0] {
        scene
            .transaction()
            .set_transform(card, Affine::translate((x, 176.0)))
            .commit()
            .unwrap();
        incremental.render_retained(&scene);
        full.render_retained(&scene);
        assert!(
            incremental.incremental_render_stats().dense_coarse_batches > 0,
            "dense backdrop damage should use bin-parallel coarse rendering"
        );
        let active_bounds = incremental
            .incremental_render_stats()
            .active_tile_bounds
            .clone();
        let incremental_image = incremental.image();
        let full_image = full.image();
        let max_difference = incremental_image
            .pixels
            .iter()
            .zip(&full_image.pixels)
            .enumerate()
            .filter(|(index, _)| {
                let x = (*index % 512) as i32;
                let y = (*index / 512) as i32;
                active_bounds.iter().any(|bounds| {
                    x >= bounds.x0 && x < bounds.x1 && y >= bounds.y0 && y < bounds.y1
                })
            })
            .flat_map(|(index, (actual, expected))| {
                actual
                    .to_le_bytes()
                    .into_iter()
                    .zip(expected.to_le_bytes())
                    .map(move |(actual, expected)| (actual.abs_diff(expected), index))
            })
            .max()
            .unwrap_or((0, 0));
        assert!(
            max_difference.0 <= 2,
            "partial liquid glass diverged inside damage at x={x} by {} channel levels at ({}, {}), active={:?}",
            max_difference.0,
            max_difference.1 % 512,
            max_difference.1 / 512,
            active_bounds,
        );
        assert!(
            incremental
                .incremental_render_stats()
                .rerendered_offscreen_tiles
                < 320,
            "a small source mutation must not rebuild all 20x16 panel tiles"
        );
    }
}

#[test]
fn persistent_retained_text_updates_dirty_glyph_allocations_and_matches_force_full() {
    if !run_wgpu_tests() {
        return;
    }
    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("Retained", 24.0));
    if layout.is_empty() {
        return;
    }
    let make_child = |x| {
        let mut canvas = Canvas::new(160, 48, 1.0);
        canvas.push_text_layout(&layout, peniko::kurbo::Point::new(x, 30.0), Color::BLACK);
        std::rc::Rc::new(canvas)
    };
    let root = RetainedNodeId::for_owner(50_400);
    let leaf = RetainedNodeId::for_owner(50_401);
    let mut scene = RetainedScene::new(160, 48, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            leaf,
            make_child(4.0),
            Affine::translate((0.0, 0.0)),
        )
        .commit()
        .unwrap();
    let mut incremental = new_test_renderer(160, 48, Color::TRANSPARENT);
    incremental.render_retained_with_text(&scene, &mut font_system, &mut text_context);
    let initial_upload = incremental.incremental_render_stats().gpu_uploaded_bytes;

    scene
        .transaction()
        .replace_scene(leaf, make_child(28.0))
        .commit()
        .unwrap();
    incremental.render_retained_with_text(&scene, &mut font_system, &mut text_context);
    assert_eq!(incremental.incremental_render_stats().chunks_rebuilt, 1);
    assert!(incremental.incremental_render_stats().reused_compiled_plan);
    assert!(incremental.incremental_render_stats().gpu_uploaded_bytes < initial_upload);

    let reference = scene.to_canvas();
    let mut full = new_test_renderer(160, 48, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render_with_text(&reference, &mut font_system, &mut text_context);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn text_clip_survives_retained_translation_and_surface_resize() {
    if !run_wgpu_tests() {
        return;
    }
    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("MMMMMMMM", 28.0));
    if layout.is_empty() {
        return;
    }

    let origin = peniko::kurbo::Point::new(8.0, 32.0);
    let clip = Rect::new(0.0, 0.0, 37.0, 48.0);
    let mut child = Canvas::new(37, 48, 1.0);
    child.push_text_layout_clipped(&layout, origin, clip, Color::BLACK);
    let root = RetainedNodeId::for_owner(50_410);
    let leaf = RetainedNodeId::for_owner(50_411);
    let mut scene = RetainedScene::new(192, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            leaf,
            std::rc::Rc::new(child),
            Affine::translate((41.0, 8.0)),
        )
        .commit()
        .unwrap();

    let mut retained = new_test_renderer(192, 64, Color::TRANSPARENT);
    retained.render_retained_with_text(&scene, &mut font_system, &mut text_context);
    assert!((0..64).all(|y| (78..192).all(|x| retained.image().rgba8_at(x, y)[3] == 0)));
    scene
        .transaction()
        .resize(152, 64, 1.0)
        .set_transform(leaf, Affine::translate((1.0, 8.0)))
        .commit()
        .unwrap();
    retained.render_retained_with_text(&scene, &mut font_system, &mut text_context);
    // Regression: translating a clipped retained text draw during resize must not make the
    // visible suffix depend on the 16px tile containing the new right edge.
    assert!((0..64).all(|y| (38..152).all(|x| retained.image().rgba8_at(x, y)[3] == 0)));

    let mut expected = Canvas::new(152, 64, 1.0);
    expected.push_text_layout_clipped(
        &layout,
        origin + (1.0, 8.0),
        Rect::new(1.0, 8.0, 38.0, 56.0),
        Color::BLACK,
    );
    let mut reference = new_test_renderer(152, 64, Color::TRANSPARENT);
    reference.render_with_text(&expected, &mut font_system, &mut text_context);

    assert_eq!(retained.image().pixels, reference.image().pixels);
}
