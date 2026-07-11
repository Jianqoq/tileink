use peniko::{
    Color, Compose, Extend, Gradient, Mix,
    kurbo::{Affine, BezPath, Line, Point, Rect, Shape},
};

use super::{Renderer, RendererOptions, WgpuRenderTargetId};
use crate::wgpu::coarse::force_coarse_emit_chunks_for_test;
use crate::wgpu::commands::WgpuCommandBatch;
use crate::{
    Canvas, FillRule, Image, ImageKey, PatternSampling, RetainedLayerDescriptor, RetainedNodeId,
    RetainedParent, RetainedScene, TextContext, TextFontSystem, TextLayoutOptions,
    debug::{RenderDebugOptions, RenderOptions},
    shared::{
        bounds::Bounds,
        brush::Brush,
        execution::ExecOp,
        gpu_coarse::{FineTileKind, PtclRecord},
        layer::{
            filter::{
                BlurSampling, COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE,
                ColorChannel, CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting,
                DisplacementMap, Filter, FilterInput, FilterPrimitive, FilterPrimitiveKind,
                LightSource, MorphologyOperator, RectLiquidGlass, SpecularLighting, Turbulence,
                TurbulenceKind,
            },
            mask::{Mask, MaskKind},
            region::Region,
        },
        tile_seg_range::TileSegmentRange,
    },
};

#[test]
fn persistent_retained_scene_updates_incrementally_and_reuses_static_frames() {
    if !run_wgpu_tests() {
        return;
    }

    let child = |color| {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        std::sync::Arc::new(canvas)
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
        (0.0, 0.0),
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
            std::sync::Arc::new(leaf),
            (0.0, 0.0),
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
        .set_position(node, (2.0, 0.0))
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
            std::sync::Arc::new(background),
            (0.0, 0.0),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            card_id,
            std::sync::Arc::new(card),
            (8.0, 8.0),
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(96, 48, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .set_position(card_id, (48.0, 8.0))
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
            std::sync::Arc::new(child),
            (0.0, 0.0),
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
    assert_profile_missing(&profile, "retained.damage.propagate");
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
            std::sync::Arc::new(child),
            (0.0, 0.0),
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
    let leaf = std::sync::Arc::new(leaf);
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
                (x, y),
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
        std::sync::Arc::new(canvas)
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
        std::sync::Arc::new(canvas)
    };
    let mut scene = RetainedScene::new(16, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(RetainedParent::content(root), None, node, plain, (0.0, 0.0))
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
        std::sync::Arc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    transaction
        .insert_scene(
            RetainedParent::content(root),
            None,
            back,
            solid(Color::from_rgb8(220, 30, 40)),
            (0.0, 0.0),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            front,
            solid(Color::from_rgb8(30, 60, 220)),
            (0.0, 0.0),
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
    transaction.replace_scene(back, std::sync::Arc::new(longer));
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
        std::sync::Arc::new(canvas)
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
        std::sync::Arc::new(canvas)
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
            ((index % 32) as f64 * 8.0, (index / 32) as f64 * 8.0),
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

    let image = std::sync::Arc::new(Image::from_rgba8(
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
        std::sync::Arc::new(canvas)
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
            ((index % 32) as f64 * 8.0, (index / 32) as f64 * 8.0),
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
            std::sync::Arc::new(child),
            (0.0, 0.0),
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
fn fine_image_bind_group_cache_invalidates_when_atlas_grows() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(20_001);
    let mut canvas = Canvas::new(8, 8, 1.0);
    canvas
        .push_image_key(
            Rect::new(0.0, 0.0, 8.0, 8.0),
            key,
            Extend::Pad,
            PatternSampling::Nearest,
        )
        .unwrap();
    let mut renderer = new_test_renderer(8, 8, Color::TRANSPARENT);
    assert!(renderer.insert_image(key, Image::from_rgba8(1, 1, [230, 20, 30, 255])));
    renderer.render(&canvas);
    assert_eq!(renderer.image().rgba8_at(4, 4), [230, 20, 30, 255]);

    let blue = [30, 80, 240, 255].repeat(64 * 64);
    assert!(renderer.insert_image(key, Image::from_rgba8(64, 64, blue)));
    renderer.render(&canvas);
    assert_eq!(renderer.image().rgba8_at(4, 4), [30, 80, 240, 255]);
}

#[test]
fn coarse_bind_group_cache_invalidates_after_scene_buffer_growth() {
    if !run_wgpu_tests() {
        return;
    }

    let mut renderer = new_test_renderer(32, 32, Color::TRANSPARENT);
    let mut small = Canvas::new(32, 32, 1.0);
    small.push_rect(
        Rect::new(0.0, 0.0, 32.0, 32.0),
        crate::Radius::ZERO,
        Color::from_rgb8(220, 30, 40),
    );
    renderer.render(&small);
    assert_eq!(renderer.image().rgba8_at(16, 16), [220, 30, 40, 255]);

    let mut grown = Canvas::new(32, 32, 1.0);
    for index in 0..512 {
        let x = (index % 32) as f64;
        let y = (index / 32) as f64;
        grown.push_rect(
            Rect::new(x, y, x + 1.0, y + 1.0),
            crate::Radius::ZERO,
            Color::BLACK,
        );
    }
    grown.push_rect(
        Rect::new(0.0, 0.0, 32.0, 32.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 80, 240),
    );
    renderer.render(&grown);
    assert_eq!(renderer.image().rgba8_at(16, 16), [30, 80, 240, 255]);
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
        std::sync::Arc::new(canvas)
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
        .replace_scene(leaf, std::sync::Arc::new(longer))
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
        .replace_scene(leaf, std::sync::Arc::new(same_shape))
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
            std::sync::Arc::new(child),
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
            (0.0, 0.0),
        )
        .insert_scene(
            RetainedParent::mask(mask),
            None,
            mask_leaf,
            solid(Color::from_rgb8(40, 220, 60)),
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background,
            solid(Color::from_rgb8(30, 70, 210)),
            (0.0, 0.0),
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
            (0.0, 0.0),
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
        )
        // The card is painted before the panel, so the later backdrop must sample it.
        .insert_scene(
            RetainedParent::content(root),
            None,
            card,
            solid(16, 16, Color::from_rgb8(235, 48, 38)),
            (8.0, 16.0),
        )
        // This is deliberately a Scene node containing an ordinary backdrop command. Cached UI
        // widgets use this shape instead of a RetainedLayerDescriptor::Backdrop hierarchy node.
        .insert_scene(
            RetainedParent::content(root),
            None,
            panel,
            std::sync::Arc::new(panel_canvas),
            (48.0, 8.0),
        )
        .commit()
        .unwrap();

    let mut incremental = new_test_renderer(96, 48, Color::TRANSPARENT);
    incremental.render_retained(&scene);
    scene
        .transaction()
        .set_position(card, (56.0, 16.0))
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
            std::sync::Arc::new(background_canvas),
            (0.0, 0.0),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            card,
            std::sync::Arc::new(card_canvas),
            (128.0, 176.0),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            panel,
            std::sync::Arc::new(panel_canvas),
            (96.0, 64.0),
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
            .set_position(card, (x, 176.0))
            .commit()
            .unwrap();
        incremental.render_retained(&scene);
        full.render_retained(&scene);
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
        )
        .insert_layer(RetainedParent::content(filter), None, front_layer, opacity)
        .insert_scene(
            RetainedParent::content(front_layer),
            None,
            front,
            solid(Color::from_rgb8(30, 60, 220)),
            (0.0, 0.0),
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
            std::sync::Arc::new(child),
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            front,
            child(Color::from_rgb8(30, 60, 220)),
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (16.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
        )
        .insert_scene(
            RetainedParent::content(mask_layer),
            None,
            moving,
            child(Color::WHITE),
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            child(Color::from_rgb8(220, 30, 40)),
            (0.0, 0.0),
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
            (16.0, 0.0),
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
            (16.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
    };
    let mut scene = RetainedScene::new(32, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            child(Color::from_rgb8(30, 70, 180)),
            (16.0, 0.0),
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            solid(
                Rect::new(24.0, 0.0, 64.0, 16.0),
                Color::from_rgb8(30, 70, 180),
            ),
            (0.0, 0.0),
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
fn persistent_retained_appends_layer_fragment_after_existing_layer() {
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
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
            (0.0, 0.0),
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
        std::sync::Arc::new(canvas)
    };
    let mut scene = RetainedScene::new(16, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            leaf,
            child(1),
            (0.0, 0.0),
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
const GPU_PTCL_END: u32 = 0;
const GPU_PTCL_COLOR: u32 = 2;
const GPU_PTCL_BEGIN_CLIP: u32 = 3;
const GPU_PTCL_END_CLIP: u32 = 4;
const GPU_PTCL_SDF: u32 = 9;
const GPU_PTCL_BEGIN_SDF_CLIP: u32 = 12;
const GPU_PTCL_IMAGE: u32 = 13;

struct ForceCoarseChunksGuard {
    previous: bool,
}

impl ForceCoarseChunksGuard {
    fn new() -> Self {
        Self {
            previous: force_coarse_emit_chunks_for_test(true),
        }
    }
}

impl Drop for ForceCoarseChunksGuard {
    fn drop(&mut self) {
        force_coarse_emit_chunks_for_test(self.previous);
    }
}

#[test]
fn wgpu_renderer_lazily_initializes_and_reuses_compute_pipelines() {
    if !run_wgpu_tests() {
        return;
    }

    let mut path = BezPath::new();
    path.move_to((2.0, 2.0));
    path.line_to((30.0, 4.0));
    path.line_to((12.0, 30.0));
    path.close_path();
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_path(
        path,
        Color::from_rgb8(40, 120, 220),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = new_test_renderer(32, 32, Color::TRANSPARENT);
    if renderer.scan_pipeline.is_none()
        || renderer.cumsum.is_none()
        || renderer.coarse_pipeline.is_none()
        || renderer.fine.is_none()
        || renderer.filter.is_none()
    {
        return;
    }
    assert_eq!(initialized_compute_pipeline_counts(&renderer), [0; 4]);
    assert_eq!(renderer.pipeline_compilation_epoch(), 0);

    renderer.render(&canvas);
    let first_render = initialized_compute_pipeline_counts(&renderer);
    let first_epoch = renderer.pipeline_compilation_epoch();
    assert!(
        first_render[..4].iter().all(|count| *count > 0),
        "tile stages should initialize only the pipelines used by the first render: {first_render:?}"
    );
    assert!(first_epoch > 0);
    renderer.render(&canvas);
    assert_eq!(initialized_compute_pipeline_counts(&renderer), first_render);
    assert_eq!(renderer.pipeline_compilation_epoch(), first_epoch);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn wgpu_renderer_populates_and_reloads_pipeline_cache_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let instance = ::wgpu::Instance::new(::wgpu::InstanceDescriptor {
        backends: ::wgpu::Backends::VULKAN,
        flags: ::wgpu::InstanceFlags::empty(),
        memory_budget_thresholds: ::wgpu::MemoryBudgetThresholds::default(),
        backend_options: ::wgpu::BackendOptions::default(),
        display: None,
    });
    let Ok(adapter) =
        pollster::block_on(instance.request_adapter(&::wgpu::RequestAdapterOptions {
            power_preference: ::wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
    else {
        return;
    };
    if !adapter
        .features()
        .contains(::wgpu::Features::PIPELINE_CACHE)
    {
        return;
    }
    let optional = ::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        | ::wgpu::Features::TEXTURE_BINDING_ARRAY
        | ::wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
    let required_features = ::wgpu::Features::PIPELINE_CACHE | (adapter.features() & optional);
    let Ok((device, queue)) =
        pollster::block_on(adapter.request_device(&::wgpu::DeviceDescriptor {
            label: Some("tileink pipeline cache test device"),
            required_features,
            required_limits: adapter.limits(),
            memory_hints: ::wgpu::MemoryHints::Performance,
            trace: ::wgpu::Trace::Off,
            experimental_features: ::wgpu::ExperimentalFeatures::disabled(),
        }))
    else {
        return;
    };
    // SAFETY: no initial cache data is supplied.
    let cache = unsafe {
        device.create_pipeline_cache(&::wgpu::PipelineCacheDescriptor {
            label: Some("tileink pipeline cache test"),
            data: None,
            fallback: true,
        })
    };

    let mut canvas = Canvas::new(64, 48, 1.0);
    canvas.push_filter_layer(
        Filter::Blur {
            std_dev_x: 2.0,
            std_dev_y: 2.0,
            sampling: BlurSampling::FULL_RES,
        },
        Region::rect(Rect::new(4.0, 4.0, 60.0, 44.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(12.0, 10.0, 52.0, 38.0),
        crate::Radius::all(5.0),
        Color::from_rgb8(40, 120, 220),
    );
    canvas.pop_layer();

    let mut renderer = Renderer::new_with_options(
        &device,
        &queue,
        64,
        48,
        Color::TRANSPARENT,
        RendererOptions {
            pipeline_cache: Some(cache.clone()),
        },
    );
    renderer.render(&canvas);
    let data = cache
        .get_data()
        .filter(|data| !data.is_empty())
        .expect("rendered compute pipelines should populate the Vulkan cache");

    // SAFETY: `data` came directly from a wgpu pipeline cache for this same device.
    let reloaded = unsafe {
        device.create_pipeline_cache(&::wgpu::PipelineCacheDescriptor {
            label: Some("tileink reloaded pipeline cache test"),
            data: Some(&data),
            fallback: false,
        })
    };
    let mut renderer = Renderer::new_with_options(
        &device,
        &queue,
        64,
        48,
        Color::TRANSPARENT,
        RendererOptions {
            pipeline_cache: Some(reloaded.clone()),
        },
    );
    renderer.render(&canvas);
    assert!(reloaded.get_data().is_some());
}

#[test]
fn wgpu_renderer_reads_native_render_output_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(8, 8, 1.0);
    canvas.push_path(
        Rect::new(2.0, 2.0, 6.0, 6.0).to_path(0.0),
        Color::from_rgb8(220, 64, 72),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    canvas.push_path(
        Rect::new(6.0, 0.0, 8.0, 2.0).to_path(0.0),
        Color::from_rgb8(32, 96, 160),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = new_test_renderer(8, 8, Color::TRANSPARENT);

    renderer.render(&canvas);
    let image = renderer.image();

    assert!(renderer.scene_buffers.draw_records_capacity() >= 8);
    assert_eq!(image.rgba8_at(3, 3), [220, 64, 72, 255]);
    assert_eq!(image.rgba8_at(0, 0), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_push_image_samples_external_image_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(4, 2, 1.0);
    canvas
        .push_image(
            Rect::new(0.0, 0.0, 4.0, 2.0),
            Image::from_rgba8(
                2,
                1,
                [
                    255, 0, 0, 255, //
                    0, 0, 255, 128,
                ],
            ),
            Extend::Pad,
            PatternSampling::Nearest,
        )
        .expect("push image");

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(1, 1), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(3, 1), [0, 0, 128, 128]);
}

#[test]
fn wgpu_renderer_push_image_key_samples_resource_buffer_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(9);
    let mut canvas = Canvas::new(4, 2, 1.0);
    canvas
        .push_image_key(
            Rect::new(0.0, 0.0, 4.0, 2.0),
            key,
            Extend::Pad,
            PatternSampling::Nearest,
        )
        .expect("push image resource");

    let mut renderer = new_test_renderer(4, 2, Color::TRANSPARENT);
    assert!(renderer.insert_image(
        key,
        Image::from_rgba8(
            2,
            1,
            [
                255, 0, 0, 255, //
                0, 0, 255, 128,
            ],
        )
    ));
    renderer.prepare_scene(&canvas);
    assert!(
        renderer.render_prepared_tile_plan(&canvas),
        "expected canvas to render through native wgpu path"
    );
    let image = renderer.image();

    assert_eq!(image.rgba8_at(1, 1), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(3, 1), [0, 0, 128, 128]);
}

#[test]
fn wgpu_renderer_push_image_key_bilinear_uses_resource_atlas() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(11);
    // Scale two source texels into one destination pixel so its center maps
    // exactly to the source texel boundary and must interpolate both colors.
    let mut canvas = Canvas::new(1, 1, 1.0);
    canvas
        .push_image_key(
            Rect::new(0.0, 0.0, 1.0, 1.0),
            key,
            Extend::Pad,
            PatternSampling::Bilinear,
        )
        .expect("push image resource");

    let mut renderer = new_test_renderer(1, 1, Color::TRANSPARENT);
    assert!(renderer.insert_image(
        key,
        Image::from_rgba8(
            2,
            1,
            [
                255, 0, 0, 255, //
                0, 0, 255, 255,
            ],
        )
    ));
    renderer.prepare_scene(&canvas);
    assert!(
        renderer.render_prepared_tile_plan(&canvas),
        "expected canvas to render through native wgpu path"
    );
    let image = renderer.image();

    assert_eq!(image.rgba8_at(0, 0), [128, 0, 128, 255]);
}

#[test]
fn wgpu_renderer_push_image_key_large_image_uses_texture_table() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(17);
    let mut canvas = Canvas::new(2, 1, 1.0);
    canvas
        .push_image_key(
            Rect::new(0.0, 0.0, 2.0, 1.0),
            key,
            Extend::Pad,
            PatternSampling::Nearest,
        )
        .expect("push image resource");

    let mut renderer = new_test_renderer(2, 1, Color::TRANSPARENT);
    if renderer.image_resource_texture_table_len == 0 {
        return;
    }
    assert!(renderer.insert_image(key, red_blue_strip_image(2050, 1)));
    renderer.prepare_scene(&canvas);
    assert!(matches!(
        renderer.image_resource_upload.image_placement(
            crate::shared::image_resource::ImageResourceId::renderer(key)
        ),
        Some(crate::shared::image_resource::ImageResourcePlacement::Texture(_))
    ));
    assert!(
        renderer.render_prepared_tile_plan(&canvas),
        "expected canvas to render through native wgpu path"
    );
    let image = renderer.image();

    assert_eq!(image.rgba8_at(0, 0), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
}

#[test]
fn wgpu_renderer_resource_atlas_nearest_repeat_samples_wrapped_pixels() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(12);
    let mut canvas = Canvas::new(4, 1, 1.0);
    let brush = Brush::from_image_key_with_options(
        key,
        Rect::new(0.0, 0.0, 2.0, 1.0),
        Extend::Repeat,
        PatternSampling::Nearest,
        255,
    )
    .expect("resource brush");
    canvas.push_rect(Rect::new(0.0, 0.0, 4.0, 1.0), crate::Radius::ZERO, brush);

    let image = render_resource_atlas_test(canvas, key);

    assert_eq!(image.rgba8_at(0, 0), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
    assert_eq!(image.rgba8_at(2, 0), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(3, 0), [0, 0, 255, 255]);
}

#[test]
fn wgpu_renderer_resource_atlas_nearest_reflect_samples_mirrored_pixels() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(13);
    let mut canvas = Canvas::new(6, 1, 1.0);
    let brush = Brush::from_image_key_with_options(
        key,
        Rect::new(0.0, 0.0, 2.0, 1.0),
        Extend::Reflect,
        PatternSampling::Nearest,
        255,
    )
    .expect("resource brush");
    canvas.push_rect(Rect::new(0.0, 0.0, 6.0, 1.0), crate::Radius::ZERO, brush);

    let image = render_resource_atlas_test(canvas, key);

    assert_eq!(image.rgba8_at(0, 0), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
    assert_eq!(image.rgba8_at(2, 0), [0, 0, 255, 255]);
    assert_eq!(image.rgba8_at(3, 0), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(4, 0), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(5, 0), [0, 0, 255, 255]);
}

#[test]
fn wgpu_renderer_resource_atlas_bilinear_repeat_samples_wrapped_pixels() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(14);
    let mut canvas = Canvas::new(2, 1, 1.0);
    // Shift the pattern by half a destination pixel. Both destination centers
    // then land halfway between source texels, including across the repeat
    // seam for the second pixel.
    let brush = Brush::from_image_key_with_options(
        key,
        Rect::new(-0.5, 0.0, 1.5, 1.0),
        Extend::Repeat,
        PatternSampling::Bilinear,
        255,
    )
    .expect("resource brush");
    canvas.push_rect(Rect::new(0.0, 0.0, 2.0, 1.0), crate::Radius::ZERO, brush);

    let image = render_resource_atlas_test(canvas, key);

    assert_eq!(image.rgba8_at(0, 0), [128, 0, 128, 255]);
    assert_eq!(image.rgba8_at(1, 0), [128, 0, 128, 255]);
}

fn render_resource_atlas_test(canvas: Canvas, key: ImageKey) -> Image {
    let mut renderer = new_test_renderer(
        canvas.physical_width(),
        canvas.physical_height(),
        Color::TRANSPARENT,
    );
    assert!(renderer.insert_image(
        key,
        Image::from_rgba8(
            2,
            1,
            [
                255, 0, 0, 255, //
                0, 0, 255, 255,
            ],
        )
    ));
    renderer.prepare_scene(&canvas);
    assert!(
        renderer.render_prepared_tile_plan(&canvas),
        "expected canvas to render through native wgpu path"
    );
    renderer.image().clone()
}

fn red_blue_strip_image(width: u32, height: u32) -> Image {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            if x < width / 2 {
                rgba.extend_from_slice(&[255, 0, 0, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 255, 255]);
            }
        }
    }
    Image::from_rgba8(width, height, rgba)
}

#[test]
fn wgpu_renderer_push_image_key_stops_sampling_after_resource_remove_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(10);
    let mut canvas = Canvas::new(2, 1, 1.0);
    canvas
        .push_image_key(
            Rect::new(0.0, 0.0, 2.0, 1.0),
            key,
            Extend::Pad,
            PatternSampling::Nearest,
        )
        .expect("push image resource");

    let mut renderer = new_test_renderer(2, 1, Color::TRANSPARENT);
    assert!(renderer.insert_image(key, Image::from_rgba8(1, 1, [255, 0, 0, 255])));
    renderer.prepare_scene(&canvas);
    assert!(renderer.render_prepared_tile_plan(&canvas));
    assert_eq!(renderer.image().rgba8_at(0, 0), [255, 0, 0, 255]);

    assert!(renderer.remove_image(key));
    assert!(!renderer.remove_image(key));
    renderer.prepare_scene(&canvas);
    assert!(renderer.render_prepared_tile_plan(&canvas));
    assert_eq!(renderer.image().rgba8_at(0, 0), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_reuses_image_resource_upload_when_resources_are_unchanged() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(15);
    let canvas = Canvas::new(1, 1, 1.0);
    let mut renderer = new_test_renderer(1, 1, Color::TRANSPARENT);
    assert!(renderer.insert_image(key, Image::from_rgba8(1, 1, [255, 0, 0, 255])));

    renderer.prepare_image_resource_buffers(canvas.scene_image_resources(), false);
    assert!(
        !renderer.image_resource_upload.atlas_pages()[0]
            .pixels
            .is_empty()
    );
    renderer.image_resource_upload.atlas_pages_mut()[0].pixels[0] = 0xdead_beef;

    renderer.prepare_image_resource_buffers(canvas.scene_image_resources(), false);

    assert_eq!(
        renderer.image_resource_upload.atlas_pages()[0].pixels[0],
        0xdead_beef
    );
}

#[test]
fn wgpu_renderer_rebuilds_image_resource_upload_after_renderer_image_change() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(16);
    let canvas = Canvas::new(1, 1, 1.0);
    let mut renderer = new_test_renderer(1, 1, Color::TRANSPARENT);
    assert!(renderer.insert_image(key, Image::from_rgba8(1, 1, [255, 0, 0, 255])));
    renderer.prepare_image_resource_buffers(canvas.scene_image_resources(), false);
    renderer.image_resource_upload.atlas_pages_mut()[0].pixels[0] = 0xdead_beef;

    assert!(renderer.insert_image(key, Image::from_rgba8(1, 1, [0, 255, 0, 255])));
    renderer.prepare_image_resource_buffers(canvas.scene_image_resources(), false);

    assert_ne!(
        renderer.image_resource_upload.atlas_pages()[0].pixels[0],
        0xdead_beef
    );
}

#[test]
fn wgpu_renderer_reuses_pipelines_when_clear_changes() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(8, 8, 1.0);
    canvas.push_rect(
        Rect::new(2.0, 2.0, 6.0, 6.0),
        crate::Radius::ZERO,
        Color::from_rgb8(220, 64, 72),
    );
    let mut renderer = new_test_renderer(8, 8, Color::from_rgb8(10, 20, 30));

    renderer.render(&canvas);
    assert_eq!(renderer.image().rgba8_at(0, 0), [10, 20, 30, 255]);

    renderer.set_clear_color(Color::from_rgb8(7, 8, 9));
    renderer.render(&canvas);
    let image = renderer.image();
    assert_eq!(image.rgba8_at(0, 0), [7, 8, 9, 255]);
    assert_eq!(image.rgba8_at(3, 3), [220, 64, 72, 255]);
}

#[test]
fn wgpu_renderer_profile_includes_cpu_prepare_and_gpu_stages() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(
        Rect::new(2.0, 2.0, 14.0, 14.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 120, 220),
    );
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);

    renderer.start_profile();
    renderer.render(&canvas);
    let profile = renderer.end_profile().clone();

    assert_eq!(renderer.image().rgba8_at(8, 8), [30, 120, 220, 255]);
    assert!(profile.cpu_time() > std::time::Duration::ZERO);
    assert_profile_has(&profile, "prepare");
    assert_profile_has(&profile, "prepare.compile");
    assert_profile_has(&profile, "scan");
    assert_profile_has(&profile, "coarse");
    assert_profile_has(&profile, "fine");
    let incremental = profile
        .incremental_stats()
        .expect("profile must capture incremental diagnostics");
    assert_eq!(incremental.active_tiles, vec![0]);
    assert_eq!(
        incremental.active_tile_bounds,
        vec![Bounds::new(0, 0, 16, 16)]
    );
    if renderer
        .device()
        .features()
        .contains(::wgpu::Features::TIMESTAMP_QUERY)
    {
        renderer
            .device()
            .poll(::wgpu::PollType::wait_indefinitely())
            .expect("poll wgpu device for async profile readback");
        let profile = renderer.poll_profile().clone();
        assert!(
            profile
                .entries()
                .iter()
                .any(|entry| entry.gpu_duration.is_some()),
            "expected at least one GPU timestamp entry"
        );
    }
}

#[test]
fn persistent_retained_profile_breaks_out_materialization_stages() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(302);
    let child = RetainedNodeId::for_owner(303);
    let leaf = |color| {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(Rect::new(2.0, 2.0, 14.0, 14.0), crate::Radius::ZERO, color);
        std::sync::Arc::new(canvas)
    };
    let mut scene = RetainedScene::new(16, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            (0.0, 0.0),
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    renderer.render_retained(&scene);
    scene
        .transaction()
        .replace_scene(child, leaf(Color::BLACK))
        .commit()
        .unwrap();

    renderer.start_profile();
    renderer.render_retained(&scene);
    let profile = renderer.end_profile().clone();

    for stage in [
        "retained.materialize",
        "retained.materialize.analysis",
        "retained.materialize.chunks",
        "retained.materialize.plan_sync",
        "retained.materialize.frame",
    ] {
        assert_profile_has(&profile, stage);
    }
}

#[test]
fn wgpu_renderer_rejects_copy_only_texture_without_storage() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(8, 8, 1.0);
    canvas.push_path(
        Rect::new(0.0, 0.0, 8.0, 8.0).to_path(0.0),
        Color::from_rgb8(10, 20, 30),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = new_test_renderer(8, 8, Color::TRANSPARENT);
    let texture = renderer
        .device()
        .create_texture(&::wgpu::TextureDescriptor {
            label: Some("tileink wgpu renderer test texture"),
            size: ::wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ::wgpu::TextureDimension::D2,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            usage: ::wgpu::TextureUsages::COPY_DST | ::wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

    renderer
        .render_to_wgpu_texture(&canvas, &texture)
        .expect_err("copy-only texture should be rejected without CPU fallback");
}

#[test]
fn wgpu_renderer_renders_tile_fine_directly_to_storage_texture_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(8, 8, 1.0);
    canvas.push_path(
        Rect::new(0.0, 0.0, 8.0, 8.0).to_path(0.0),
        Color::from_rgb8(40, 100, 220),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = new_test_renderer(8, 8, Color::TRANSPARENT);
    if !renderer
        .device()
        .features()
        .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        return;
    }
    let texture = renderer
        .device()
        .create_texture(&::wgpu::TextureDescriptor {
            label: Some("tileink wgpu renderer direct storage texture test"),
            size: ::wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ::wgpu::TextureDimension::D2,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            usage: ::wgpu::TextureUsages::STORAGE_BINDING | ::wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

    renderer
        .render_to_wgpu_texture(&canvas, &texture)
        .expect("render directly to wgpu storage texture");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 8, 8);

    assert_eq!(
        &bytes[4 * (3 * 8 + 3)..4 * (3 * 8 + 4)],
        &[40, 100, 220, 255]
    );
}

#[test]
fn wgpu_renderer_renders_tile_fine_to_storage_texture_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(8, 8, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(12, 34, 56),
    );
    let mut renderer = new_test_renderer(8, 8, Color::TRANSPARENT);
    if !renderer
        .device()
        .features()
        .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        return;
    }
    let texture = renderer
        .device()
        .create_texture(&::wgpu::TextureDescriptor {
            label: Some("tileink wgpu renderer tile fine storage texture test"),
            size: ::wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ::wgpu::TextureDimension::D2,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            usage: ::wgpu::TextureUsages::STORAGE_BINDING | ::wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

    renderer
        .render_to_wgpu_texture(&canvas, &texture)
        .expect("render tile fine to wgpu storage texture");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 8, 8);

    assert_eq!(&bytes[4 * (3 * 8 + 3)..4 * (3 * 8 + 4)], &[12, 34, 56, 255]);
}

#[test]
fn wgpu_renderer_portable_fine_preserves_previous_batches_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let Some((device, queue)) = shared_wgpu_test_device(true) else {
        return;
    };
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_rect(
        Rect::new(8.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    let mut renderer = Renderer::new(device, queue, 16, 16, Color::TRANSPARENT);
    assert!(
        renderer
            .fine
            .as_ref()
            .is_some_and(|fine| fine.uses_portable_textures())
    );

    renderer.prepare_scene(&canvas);
    let mut commands =
        WgpuCommandBatch::new(renderer.device(), renderer.queue(), "portable fine batches");
    assert!(renderer.scan_and_cumsum(&mut commands, &canvas));
    assert!(renderer.clear_render_target(&mut commands, WgpuRenderTargetId::Main, 0));
    assert!(renderer.coarse_and_fine_batch_to(&mut commands, 0, 1, 0, 0, WgpuRenderTargetId::Main));
    assert!(renderer.coarse_and_fine_batch_to(&mut commands, 1, 2, 0, 0, WgpuRenderTargetId::Main));
    commands.finish();

    let image = renderer.image();
    assert_eq!(image.rgba8_at(4, 8), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 255, 255]);
}

#[test]
fn wgpu_renderer_renders_offscreen_plan_directly_to_storage_texture() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    canvas.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    if !renderer
        .device()
        .features()
        .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        return;
    }
    let texture = renderer
        .device()
        .create_texture(&::wgpu::TextureDescriptor {
            label: Some("tileink wgpu renderer offscreen storage texture test"),
            size: ::wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ::wgpu::TextureDimension::D2,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            usage: ::wgpu::TextureUsages::STORAGE_BINDING | ::wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

    renderer
        .render_to_wgpu_texture(&canvas, &texture)
        .expect("render offscreen plan directly to wgpu storage texture");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 16, 16);

    assert_eq!(
        &bytes[4 * (3 * 16 + 3)..4 * (3 * 16 + 4)],
        &[0, 255, 255, 255]
    );
    assert_eq!(
        &bytes[4 * (3 * 16 + 12)..4 * (3 * 16 + 13)],
        &[0, 255, 0, 255]
    );
}

#[test]
fn wgpu_renderer_debug_capture_uses_native_scan_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_path(
        Rect::new(4.0, 4.0, 20.0, 20.0).to_path(0.1),
        Color::from_rgb8(0, 128, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    let options = RenderOptions {
        debug: Some(RenderDebugOptions::new("target/wgpu-debug-capture-test").with_tile((0, 0))),
    };
    let mut renderer = new_test_renderer(32, 32, Color::TRANSPARENT);

    let capture = renderer.render_with_options(&canvas, &options);

    assert_eq!(capture.backend, "wgpu");
    assert_eq!(capture.tiles.len(), 4);
    assert!(
        capture
            .tile
            .as_ref()
            .is_some_and(|tile| !tile.paths.is_empty())
    );
    assert!(capture.images.iter().any(|image| image.name == "final.png"));
}

#[test]
fn wgpu_renderer_renders_sdf_primitives_in_fine_pass_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(
        Rect::new(2.0, 2.0, 14.0, 14.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 120, 220),
    );
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    assert!(renderer.fine.is_some());

    renderer.render(&canvas);
    let image = renderer.image();

    assert_eq!(image.rgba8_at(8, 8), [30, 120, 220, 255]);
    assert_eq!(image.rgba8_at(0, 0), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_samples_gradient_brush_in_fine_pass_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let gradient = Gradient::new_linear((0.0, 0.0), (31.0, 0.0))
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let mut canvas = Canvas::new(32, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        &gradient,
    );
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    assert!(renderer.fine.is_some());

    renderer.render(&canvas);
    let image = renderer.image();
    let left = image.rgba8_at(2, 8);
    let right = image.rgba8_at(29, 8);

    assert!(left[0] > left[2], "expected red side, got {left:?}");
    assert!(right[2] > right[0], "expected blue side, got {right:?}");
    assert_eq!(left[3], 255);
    assert_eq!(right[3], 255);
}

#[test]
fn wgpu_renderer_accumulates_many_translucent_fine_particles_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    for ix in 0u8..16 {
        let alpha = 24 + ix * 5;
        canvas.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            crate::Radius::ZERO,
            Color::from_rgba8(12 + ix * 3, 80, 220u8.saturating_sub(ix * 4), alpha),
        );
    }

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(&image, &expected, 0, "f32 fine particle accumulation");
}

#[test]
fn wgpu_scan_emits_segments_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_path(
        Line::new((4.0, 0.0), (4.0, 16.0)).to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    if renderer.scan_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    renderer.scan_for_test();

    let backdrops = renderer.scan.backdrops.read::<i32>(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.backdrop_len,
    );
    let ranges = renderer.scan.tile_segment_ranges.read::<TileSegmentRange>(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.backdrop_len,
    );
    let segment_bumps = renderer.scan.segment_bumps.read::<u32>(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.path_count,
    );
    let segments = renderer
        .scan
        .segments
        .read::<crate::shared::line_seg::LineSegment>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.segment_capacity,
        );

    assert_eq!(backdrops, vec![0]);
    assert_eq!(ranges, vec![TileSegmentRange { start: 0, end: 2 }]);
    assert_eq!(segment_bumps, vec![2]);
    assert!((segments[0].p0x - 4.0).abs() < 1e-3);
    assert!((segments[0].p1y - 16.0).abs() < 1e-6);
}

#[test]
fn wgpu_cumsum_scans_backdrop_rows_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(48, 32, 1.0);
    canvas.push_path(
        Rect::new(0.0, 0.0, 48.0, 32.0).to_path(0.0),
        Color::BLACK,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = new_test_renderer(48, 32, Color::TRANSPARENT);
    if renderer.cumsum.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    let device = renderer.device().clone();
    let queue = renderer.queue().clone();
    renderer.scan.backdrops.upload(
        &device,
        &queue,
        "tileink wgpu cumsum test backdrops",
        &[1, -1, 2, 3, 0, -2],
    );
    renderer.cumsum_for_test();

    assert_eq!(
        renderer.scan.backdrops.read::<i32>(
            renderer.device(),
            renderer.queue(),
            renderer.lengths.backdrop_len
        ),
        vec![1, 0, 2, 3, 3, 1]
    );
}

#[test]
fn wgpu_coarse_emits_sdf_particles_for_rects_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(32, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.25, 0.25, 31.75, 15.75),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_rect(
        Rect::new(16.25, 0.25, 31.75, 15.75),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    renderer.coarse_batch(&canvas, 0, canvas.draw_records.len() as u32, 0, 0);

    let tile_records = renderer.coarse.read_tile_records(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.tile_count,
    );
    assert_eq!(
        tile_records
            .iter()
            .map(|record| record.ptcl_start)
            .collect::<Vec<_>>(),
        vec![0, 2]
    );
    assert_eq!(
        tile_records
            .iter()
            .map(|record| record.ptcl_end)
            .collect::<Vec<_>>(),
        vec![2, 5]
    );
    assert_eq!(
        read_ptcl_tags(&renderer, 5),
        vec![
            GPU_PTCL_SDF,
            GPU_PTCL_END,
            GPU_PTCL_SDF,
            GPU_PTCL_SDF,
            GPU_PTCL_END,
        ]
    );
    assert_eq!(
        read_ptcl_colors(&renderer, renderer.lengths.coarse_ptcl_capacity),
        vec![0, 0, 0, 1, 0]
    );
}

#[test]
fn wgpu_coarse_tile_draw_bins_respect_batch_range_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(32, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.25, 0.25, 31.75, 15.75),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_rect(
        Rect::new(0.25, 0.25, 31.75, 15.75),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    let device = renderer.device().clone();
    let queue = renderer.queue().clone();
    renderer
        .scene_buffers
        .upload_test_batch_ids(&device, &queue, &[0, 1]);
    renderer.coarse_batch(&canvas, 1, 2, 0, 0);

    let tile_records = renderer.coarse.read_tile_records(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.tile_count,
    );
    assert_eq!(
        tile_records
            .iter()
            .map(|record| record.ptcl_start)
            .collect::<Vec<_>>(),
        vec![0, 2]
    );
    assert_eq!(
        tile_records
            .iter()
            .map(|record| record.ptcl_end)
            .collect::<Vec<_>>(),
        vec![2, 4]
    );
    assert_eq!(
        read_ptcl_tags(&renderer, 4),
        vec![GPU_PTCL_SDF, GPU_PTCL_END, GPU_PTCL_SDF, GPU_PTCL_END]
    );
    assert_eq!(read_ptcl_colors(&renderer, 4), vec![1, 0, 1, 0]);
}

#[test]
fn wgpu_coarse_emits_deep_inside_sdf_rect_tiles_as_solid_color_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(
        Rect::new(0.25, 0.25, 63.75, 63.75),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    let mut renderer = new_test_renderer(64, 64, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    renderer.coarse_batch(&canvas, 0, canvas.draw_records.len() as u32, 0, 0);

    let tags = read_ptcl_tags(&renderer, renderer.lengths.coarse_ptcl_capacity);
    assert_eq!(tags[0], GPU_PTCL_SDF);
    assert_eq!(tags[10], GPU_PTCL_COLOR);
}

#[test]
fn wgpu_coarse_emits_deep_inside_rounded_sdf_rect_tiles_as_solid_color_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(96, 96, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 96.0, 96.0),
        crate::Radius::all(32.0),
        Color::from_rgb8(255, 0, 0),
    );
    let mut renderer = new_test_renderer(96, 96, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    renderer.coarse_batch(&canvas, 0, canvas.draw_records.len() as u32, 0, 0);

    let tags = read_ptcl_tags(&renderer, renderer.lengths.coarse_ptcl_capacity);
    assert_eq!(tags[0], GPU_PTCL_SDF);
    assert_eq!(tags[42], GPU_PTCL_COLOR);
}

#[test]
fn wgpu_coarse_portable_emit_handles_multiple_draw_chunks_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }
    let Some((device, queue)) = shared_wgpu_test_device(true) else {
        return;
    };

    let mut canvas = Canvas::new(16, 16, 1.0);
    for _ in 0..257 {
        canvas.push_rect(
            Rect::new(0.25, 0.25, 15.75, 15.75),
            crate::Radius::ZERO,
            Color::BLACK,
        );
    }
    let mut renderer = Renderer::new(device, queue, 16, 16, Color::TRANSPARENT);
    let Some(_) = renderer.coarse_pipeline.as_ref() else {
        return;
    };

    renderer.prepare_scene(&canvas);
    assert_eq!(renderer.lengths.tile_draw_chunk_count, 2);
    renderer.coarse_batch(&canvas, 0, canvas.draw_records.len() as u32, 0, 0);

    let tile_records = renderer.coarse.read_tile_records(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.tile_count,
    );
    assert_eq!(tile_records[0].ptcl_start, 0);
    assert_eq!(tile_records[0].ptcl_end, 258);
    let tags = read_ptcl_tags(&renderer, 258);
    assert!(tags[..257].iter().all(|&tag| tag == GPU_PTCL_SDF));
    assert_eq!(tags[257], GPU_PTCL_END);
}

#[test]
fn wgpu_fine_tile_kind_classifies_analytic_tiles_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(96, 96, 1.0);
    canvas.push_rect(
        // Fractional outer edges keep boundary tiles on the analytic SDF path,
        // while interior tiles are still provably solid.
        Rect::new(0.25, 0.25, 95.75, 95.75),
        crate::Radius::ZERO,
        Color::from_rgb8(32, 64, 96),
    );
    canvas.push_rect(
        Rect::new(32.25, 32.25, 47.75, 47.75),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    let mut renderer = new_test_renderer(96, 96, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    renderer.coarse_batch(&canvas, 0, canvas.draw_records.len() as u32, 0, 0);

    let kinds =
        renderer
            .coarse
            .read_fine_tile_kinds(renderer.device(), renderer.queue(), renderer.lengths);
    assert_eq!(kinds[0], FineTileKind::PureSdfSolidNoStack);
    assert_eq!(kinds[2 + 2 * 6], FineTileKind::MixedAnalyticSolidNoStack);
    assert_eq!(kinds[3 + 3 * 6], FineTileKind::ColorOnlyNoStack);
}

#[test]
fn wgpu_fine_tile_kind_classifies_full_tile_image_rect_as_analytic_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas
        .push_image(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Image::from_rgba8(1, 1, [40, 90, 180, 255]),
            Extend::Pad,
            PatternSampling::Bilinear,
        )
        .expect("push image");
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    renderer.coarse_batch(&canvas, 0, canvas.draw_records.len() as u32, 0, 0);

    let kinds =
        renderer
            .coarse
            .read_fine_tile_kinds(renderer.device(), renderer.queue(), renderer.lengths);
    assert_eq!(kinds[0], FineTileKind::PureSdfSolidNoStack);
    let tags = read_ptcl_tags(&renderer, 2);
    assert_eq!(tags, vec![GPU_PTCL_IMAGE, GPU_PTCL_END]);
}

#[test]
fn wgpu_chunked_render_classifies_full_tile_image_rect_as_analytic_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let _force_chunks = ForceCoarseChunksGuard::new();
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas
        .push_image(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Image::from_rgba8(1, 1, [40, 90, 180, 255]),
            Extend::Pad,
            PatternSampling::Bilinear,
        )
        .expect("push image");
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    if renderer.fine.is_none() {
        return;
    }

    renderer.render(&canvas);

    let kinds =
        renderer
            .coarse
            .read_fine_tile_kinds(renderer.device(), renderer.queue(), renderer.lengths);
    assert_eq!(kinds[0], FineTileKind::PureSdfSolidNoStack);
    let tags = read_ptcl_tags(&renderer, 2);
    assert_eq!(tags, vec![GPU_PTCL_IMAGE, GPU_PTCL_END]);
}

#[test]
fn wgpu_fine_tile_kind_elides_full_cover_path_clip_for_image_rect_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_clip_layer(
        Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    canvas
        .push_image(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Image::from_rgba8(1, 1, [40, 90, 180, 255]),
            Extend::Pad,
            PatternSampling::Bilinear,
        )
        .expect("push image");
    canvas.pop_layer();
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    coarse_first_draw_batch(&mut renderer, &canvas);

    let kinds =
        renderer
            .coarse
            .read_fine_tile_kinds(renderer.device(), renderer.queue(), renderer.lengths);
    assert_eq!(kinds[0], FineTileKind::PureSdfSolidNoStack);
    let tags = read_ptcl_tags(&renderer, 2);
    assert_eq!(tags, vec![GPU_PTCL_IMAGE, GPU_PTCL_END]);
}

#[test]
fn wgpu_fine_tile_kind_elides_full_cover_sdf_clip_for_image_rect_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO);
    canvas
        .push_image(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Image::from_rgba8(1, 1, [40, 90, 180, 255]),
            Extend::Pad,
            PatternSampling::Bilinear,
        )
        .expect("push image");
    canvas.pop_layer();
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    coarse_first_draw_batch(&mut renderer, &canvas);

    let kinds =
        renderer
            .coarse
            .read_fine_tile_kinds(renderer.device(), renderer.queue(), renderer.lengths);
    assert_eq!(kinds[0], FineTileKind::PureSdfSolidNoStack);
    let tags = read_ptcl_tags(&renderer, 2);
    assert_eq!(tags, vec![GPU_PTCL_IMAGE, GPU_PTCL_END]);
}

#[test]
fn wgpu_fine_tile_kind_keeps_partial_clip_image_rect_on_full_interpreter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_clip_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    canvas
        .push_image(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Image::from_rgba8(1, 1, [40, 90, 180, 255]),
            Extend::Pad,
            PatternSampling::Bilinear,
        )
        .expect("push image");
    canvas.pop_layer();
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    coarse_first_draw_batch(&mut renderer, &canvas);

    let kinds =
        renderer
            .coarse
            .read_fine_tile_kinds(renderer.device(), renderer.queue(), renderer.lengths);
    assert_eq!(kinds[0], FineTileKind::FullInterpreter);
    let tags = read_ptcl_tags(&renderer, 4);
    assert_eq!(
        tags,
        vec![
            GPU_PTCL_BEGIN_SDF_CLIP,
            GPU_PTCL_IMAGE,
            GPU_PTCL_END_CLIP,
            GPU_PTCL_END
        ]
    );
}

#[test]
fn wgpu_fine_indirect_dispatch_counts_match_tile_kinds_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(96, 96, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 96.0, 96.0),
        crate::Radius::ZERO,
        Color::from_rgb8(32, 64, 96),
    );
    canvas.push_rect(
        Rect::new(32.0, 32.0, 48.0, 48.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    let mut renderer = new_test_renderer(96, 96, Color::TRANSPARENT);
    if renderer.fine.is_none() {
        return;
    }

    renderer.render(&canvas);

    let kinds =
        renderer
            .coarse
            .read_fine_tile_kinds(renderer.device(), renderer.queue(), renderer.lengths);
    let sdf_count = kinds
        .iter()
        .filter(|&&kind| kind == FineTileKind::PureSdfSolidNoStack)
        .count() as u32;
    let mixed_count = kinds
        .iter()
        .filter(|&&kind| {
            matches!(
                kind,
                FineTileKind::ColorOnlyNoStack | FineTileKind::MixedAnalyticSolidNoStack
            )
        })
        .count() as u32;
    let full_count = kinds.len() as u32 - sdf_count - mixed_count;
    let args = renderer
        .fine_indirect_args
        .read::<u32>(renderer.device(), renderer.queue(), 9);
    assert_eq!(
        &args,
        &[sdf_count, 1, 1, mixed_count, 1, 1, full_count, 1, 1]
    );
}

#[test]
fn wgpu_fine_tile_kind_keeps_clip_tiles_on_full_interpreter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_clip_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    if renderer.coarse_pipeline.is_none() {
        return;
    }

    renderer.prepare_scene(&canvas);
    coarse_first_draw_batch(&mut renderer, &canvas);

    let kinds =
        renderer
            .coarse
            .read_fine_tile_kinds(renderer.device(), renderer.queue(), renderer.lengths);
    assert_eq!(kinds[0], FineTileKind::FullInterpreter);
}

#[test]
fn wgpu_renderer_applies_path_clip_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_clip_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);

    renderer.render(&canvas);
    let image = renderer.image();

    assert_eq!(image.rgba8_at(4, 8), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_opacity_layer_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

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
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);

    renderer.render(&canvas);
    let pixel = renderer.image().rgba8_at(8, 8);

    assert!(
        (126..=129).contains(&pixel[3]),
        "unexpected pixel {pixel:?}"
    );
    assert_eq!(pixel[1], 0);
    assert_eq!(pixel[2], 0);
}

#[test]
fn wgpu_renderer_applies_blend_layer_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(200, 80, 40));
    canvas.push_blend_layer(
        full.to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(64, 200, 180));
    canvas.pop_layer();

    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    renderer.render(&canvas);

    let expected = render_native_wgpu(&canvas);
    assert_images_near(&renderer.image(), &expected, 0, "multiply blend layer");
}

#[test]
fn wgpu_renderer_applies_color_filter_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(4, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_color_matrix_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::ColorMatrix([
            0.0, 0.0, 0.0, 0.0, 0.0, //
            1.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, 0.0,
        ]),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_component_transfer_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut table = Box::new([0; COMPONENT_TRANSFER_TABLE_LEN]);
    for i in 0..COMPONENT_TRANSFER_TABLE_SIZE {
        table[i] = 0;
        table[COMPONENT_TRANSFER_TABLE_SIZE + i] = if i == 0 { 255 } else { i as u32 };
        table[2 * COMPONENT_TRANSFER_TABLE_SIZE + i] = 0;
        table[3 * COMPONENT_TRANSFER_TABLE_SIZE + i] = i as u32;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::ComponentTransfer(table),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_convolve_matrix_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(3, 1, 1.0);
    canvas.push_filter_layer(
        Filter::ConvolveMatrix(ConvolveMatrix {
            columns: 3,
            rows: 1,
            target_x: 1,
            target_y: 0,
            data: vec![1.0, 0.0, 0.0],
            divisor: 1.0,
            bias: 0.0,
            edge_mode: ConvolveEdgeMode::Duplicate,
            preserve_alpha: false,
        }),
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(10, 0, 0),
    );
    canvas.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(20, 0, 0),
    );
    canvas.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(40, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(0, 0), [20, 0, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [40, 0, 0, 255]);
    assert_eq!(image.rgba8_at(2, 0), [40, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_diffuse_lighting_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(3, 1, 1.0);
    canvas.push_filter_layer(
        Filter::DiffuseLighting(DiffuseLighting {
            surface_scale: 1.0,
            diffuse_constant: 1.0,
            lighting_color: [1.0, 0.0, 0.0],
            light_source: LightSource::Distant {
                azimuth: 180.0,
                elevation: 0.0,
            },
        }),
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgba8(0, 0, 0, 0),
    );
    canvas.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgba8(0, 0, 0, 128),
    );
    canvas.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let center = image.rgba8_at(1, 0);

    assert!(
        center[0].abs_diff(180) <= 1 && center[1] == 0 && center[2] == 0 && center[3] == 255,
        "expected red diffuse lighting at alpha slope center, got {center:?}"
    );
}

#[test]
fn wgpu_renderer_applies_specular_lighting_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(1, 1, 1.0);
    canvas.push_filter_layer(
        Filter::SpecularLighting(SpecularLighting {
            surface_scale: 0.0,
            specular_constant: 0.5,
            specular_exponent: 1.0,
            lighting_color: [1.0, 0.5, 0.0],
            light_source: LightSource::Point {
                x: 0.5,
                y: 0.5,
                z: 1.0,
            },
        }),
        Region::rect(Rect::new(0.0, 0.0, 1.0, 1.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(0, 0), [128, 64, 0, 128]);
}

#[test]
fn wgpu_renderer_executes_filter_graph_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(8, 8, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::canvas(8, 8),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: Some(FilterInput::Primitive(0)),
                    region: Bounds::new(0, 0, 4, 8),
                    kind: FilterPrimitiveKind::Blend {
                        mode: Mix::Multiply,
                    },
                },
                FilterPrimitive {
                    input: FilterInput::Primitive(0),
                    input2: Some(FilterInput::SourceAlpha),
                    region: Bounds::new(4, 0, 8, 8),
                    kind: FilterPrimitiveKind::Composite {
                        operator: CompositeOperator::In,
                    },
                },
                FilterPrimitive {
                    input: FilterInput::Primitive(1),
                    input2: Some(FilterInput::Primitive(2)),
                    region: Bounds::canvas(8, 8),
                    kind: FilterPrimitiveKind::Composite {
                        operator: CompositeOperator::Over,
                    },
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(2, 4), [0, 0, 0, 255]);
    assert_eq!(image.rgba8_at(6, 4), [0, 0, 255, 255]);
}

#[test]
fn wgpu_renderer_applies_filter_graph_displacement_map_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(8, 2, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 8, 2),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::from_rgba8(255, 0, 0, 128)),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: Some(FilterInput::Primitive(0)),
                    region: Bounds::new(0, 0, 8, 2),
                    kind: FilterPrimitiveKind::DisplacementMap(DisplacementMap {
                        scale_x: 4.0,
                        scale_y: 0.0,
                        x_channel: ColorChannel::R,
                        y_channel: ColorChannel::A,
                        linear_rgb: false,
                    }),
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 2.0), crate::Radius::ZERO),
    );
    for x in 0..8 {
        canvas.push_rect(
            Rect::new(x as f64, 0.0, x as f64 + 1.0, 2.0),
            crate::Radius::ZERO,
            Color::from_rgb8((x as u8 + 1) * 20, 0, 0),
        );
    }
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(&image, &expected, 0, "filter graph displacement map");
}

#[test]
fn wgpu_renderer_generates_filter_graph_turbulence_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(32, 24, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![FilterPrimitive {
                input: FilterInput::SourceGraphic,
                input2: None,
                region: Bounds::new(6, 5, 28, 20),
                kind: FilterPrimitiveKind::Turbulence(Turbulence {
                    stitch_tiles: true,
                    linear_rgb: true,
                    ..test_turbulence(TurbulenceKind::FractalNoise, -20, 4)
                }),
            }],
            fixed_region: true,
        },
        Region::rect(Rect::new(4.0, 3.0, 30.0, 22.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(4.0, 3.0, 30.0, 22.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(&image, &expected, 0, "filter graph turbulence");
}

#[test]
fn wgpu_renderer_filter_graph_turbulence_uses_surface_origin_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(80, 24, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![FilterPrimitive {
                input: FilterInput::SourceGraphic,
                input2: None,
                region: Bounds::new(40, 4, 72, 20),
                kind: FilterPrimitiveKind::Turbulence(Turbulence {
                    base_frequency_x: 0.09,
                    base_frequency_y: 0.13,
                    tile_x: 40.0,
                    tile_y: 4.0,
                    tile_width: 32.0,
                    tile_height: 16.0,
                    ..test_turbulence(TurbulenceKind::Turbulence, 5, 3)
                }),
            }],
            fixed_region: true,
        },
        Region::rect(Rect::new(40.0, 4.0, 72.0, 20.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(40.0, 4.0, 72.0, 20.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(
        &image,
        &expected,
        0,
        "filter graph turbulence surface origin",
    );
}

#[test]
fn wgpu_renderer_rasterizes_path_region_mask_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut triangle = BezPath::new();
    triangle.move_to((4.0, 4.0));
    triangle.line_to((12.0, 4.0));
    triangle.line_to((4.0, 12.0));
    triangle.close_path();
    let region = Region::path(triangle, Affine::IDENTITY, 0.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_backdrop_layer(Filter::Invert(1.0), region.clone());
    canvas.pop_layer();

    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&canvas);
    assert_eq!(
        renderer
            .filter_paths
            .range_starts
            .read::<u32>(renderer.device(), renderer.queue(), 1),
        vec![0]
    );
    assert_eq!(
        renderer
            .filter_paths
            .range_ends
            .read::<u32>(renderer.device(), renderer.queue(), 1),
        vec![3]
    );
    assert_eq!(
        renderer
            .filter_paths
            .p0x
            .read::<i32>(renderer.device(), renderer.queue(), 3),
        vec![1024, 3072, 1024]
    );

    let mask = renderer.acquire_scratch().expect("scratch mask");
    let mut commands = WgpuCommandBatch::new(renderer.device(), renderer.queue(), "test mask");
    renderer.clear_render_target(&mut commands, mask, 0);
    assert!(renderer.build_region_mask(
        &mut commands,
        mask,
        &region,
        Some(0),
        Bounds::new(4, 4, 12, 12)
    ));
    commands.finish();

    let pixels = read_render_target_u32(&renderer, mask, 16 * 16);
    assert_eq!(pixels[6 * 16 + 6], 0xffffffff);
    assert_eq!(pixels[10 * 16 + 10], 0);
}

#[test]
fn wgpu_renderer_rasterizes_nonzero_path_region_mask_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut path = BezPath::new();
    for _ in 0..2 {
        path.move_to((4.0, 4.0));
        path.line_to((12.0, 4.0));
        path.line_to((4.0, 12.0));
        path.close_path();
    }
    let region = Region::path(path, Affine::IDENTITY, 0.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_backdrop_layer(Filter::Invert(1.0), region.clone());
    canvas.pop_layer();

    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&canvas);
    let mask = renderer.acquire_scratch().expect("scratch mask");
    let mut commands = WgpuCommandBatch::new(renderer.device(), renderer.queue(), "test mask");
    renderer.clear_render_target(&mut commands, mask, 0);
    assert!(renderer.build_region_mask(
        &mut commands,
        mask,
        &region,
        Some(0),
        Bounds::new(4, 4, 12, 12)
    ));
    commands.finish();

    let pixels = read_render_target_u32(&renderer, mask, 16 * 16);
    assert_eq!(pixels[6 * 16 + 6], 0xffffffff);
    assert_eq!(pixels[10 * 16 + 10], 0);
}

#[test]
fn wgpu_renderer_rect_liquid_glass_backdrop_is_stable_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(64, 32, 1.0);
    for x in 0..64 {
        let v = (x * 4) as u8;
        canvas.push_rect(
            Rect::new(f64::from(x), 0.0, f64::from(x + 1), 32.0),
            crate::Radius::ZERO,
            Color::from_rgb8(v, v, v),
        );
    }
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 6,
            blur_sampling: BlurSampling::downsampled(2),
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(16.0, 4.0, 48.0, 28.0), crate::Radius::all(6.0)),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(&image, &expected, 0, "rect liquid glass backdrop");
}

#[test]
fn wgpu_renderer_clips_rect_liquid_glass_backdrop_with_outer_sdf_clip_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(20, 40, 80),
    );
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::all(6.0));
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 0,
            tint: Color::from_rgba8(255, 0, 0, 255),
            refraction_factor: 0.0,
            fresnel_factor: 0.0,
            glare_factor: 0.0,
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.pop_layer();
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_ne!(image.rgba8_at(8, 8), [20, 40, 80, 255]);
    assert_eq!(image.rgba8_at(0, 0), [20, 40, 80, 255]);
}

#[test]
fn wgpu_renderer_clips_rect_liquid_glass_children_with_outer_sdf_clip_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(20, 40, 80),
    );
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::all(6.0));
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 0,
            tint: Color::from_rgba8(255, 0, 0, 255),
            refraction_factor: 0.0,
            fresnel_factor: 0.0,
            glare_factor: 0.0,
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    canvas.pop_layer();
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
    assert_eq!(image.rgba8_at(0, 0), [20, 40, 80, 255]);
}

#[test]
fn wgpu_renderer_merges_filter_graph_inputs_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(8, 8, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::canvas(8, 8),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 4, 8),
                    kind: FilterPrimitiveKind::Merge {
                        inputs: vec![FilterInput::Primitive(0), FilterInput::SourceGraphic],
                    },
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(2, 4), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(6, 4), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_tiles_filter_graph_input_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(6, 4, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![FilterPrimitive {
                input: FilterInput::SourceGraphic,
                input2: None,
                region: Bounds::new(0, 0, 6, 4),
                kind: FilterPrimitiveKind::Tile {
                    source_region: Bounds::new(1, 1, 3, 3),
                },
            }],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 6.0, 4.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(1.0, 1.0, 2.0, 2.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_rect(
        Rect::new(2.0, 1.0, 3.0, 2.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    canvas.push_rect(
        Rect::new(1.0, 2.0, 2.0, 3.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    canvas.push_rect(
        Rect::new(2.0, 2.0, 3.0, 3.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 255, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(0, 0), [255, 255, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
    assert_eq!(image.rgba8_at(2, 0), [255, 255, 0, 255]);
    assert_eq!(image.rgba8_at(3, 1), [255, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_displaces_filter_graph_input_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(3, 1, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 3, 1),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::WHITE),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: Some(FilterInput::Primitive(0)),
                    region: Bounds::new(0, 0, 3, 1),
                    kind: FilterPrimitiveKind::DisplacementMap(DisplacementMap {
                        scale_x: 2.0,
                        scale_y: 0.0,
                        x_channel: ColorChannel::R,
                        y_channel: ColorChannel::A,
                        linear_rgb: false,
                    }),
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    canvas.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(0, 0), [0, 255, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
    assert_eq!(image.rgba8_at(2, 0), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_solid_flood_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::Flood {
            brush: Brush::Solid(Color::from_rgba8(20, 40, 80, 128)),
        },
        Region::rect(Rect::new(4.0, 4.0, 12.0, 12.0), crate::Radius::ZERO),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(8, 8), [10, 20, 40, 128]);
    assert_eq!(image.rgba8_at(2, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_samples_gradient_flood_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let gradient = Gradient::new_linear((0.0, 0.0), (15.0, 0.0))
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::Flood {
            brush: Brush::from_gradient(&gradient),
        },
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let left = image.rgba8_at(2, 8);
    let right = image.rgba8_at(13, 8);

    assert!(left[0] > left[2], "expected red side, got {left:?}");
    assert!(right[2] > right[0], "expected blue side, got {right:?}");
    assert_eq!(left[3], 255);
    assert_eq!(right[3], 255);
}

#[test]
fn wgpu_renderer_samples_resource_image_flood_filter_with_atlas() {
    if !run_wgpu_tests() {
        return;
    }

    let key = ImageKey::new(15);
    let brush = Brush::from_image_key_with_options(
        key,
        Rect::new(0.0, 0.0, 1.0, 1.0),
        Extend::Pad,
        PatternSampling::Bilinear,
        255,
    )
    .expect("resource brush");
    let mut canvas = Canvas::new(1, 1, 1.0);
    canvas.push_filter_layer(
        Filter::Flood { brush },
        Region::rect(Rect::new(0.0, 0.0, 1.0, 1.0), crate::Radius::ZERO),
    );
    canvas.pop_layer();

    let image = render_resource_atlas_test(canvas, key);

    assert_eq!(image.rgba8_at(0, 0), [128, 0, 128, 255]);
}

#[test]
fn wgpu_renderer_applies_solid_drop_shadow_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::DropShadow {
            offset_x: 2.0,
            offset_y: 1.0,
            std_dev: 0.0,
            brush: Brush::Solid(Color::from_rgba8(0, 0, 0, 128)),
        },
        Region::rect(Rect::new(4.0, 4.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(4.0, 4.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(5, 5), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(9, 6), [0, 0, 0, 128]);
}

#[test]
fn wgpu_renderer_samples_gradient_drop_shadow_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let gradient = Gradient::new_linear((0.0, 0.0), (31.0, 0.0))
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_filter_layer(
        Filter::DropShadow {
            offset_x: 0.0,
            offset_y: 12.0,
            std_dev: 0.0,
            brush: Brush::from_gradient(&gradient),
        },
        Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 32.0, 8.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let left = image.rgba8_at(4, 16);
    let right = image.rgba8_at(27, 16);

    assert!(left[0] > left[2], "expected red shadow side, got {left:?}");
    assert!(
        right[2] > right[0],
        "expected blue shadow side, got {right:?}"
    );
    assert_eq!(left[3], 255);
    assert_eq!(right[3], 255);
    assert_eq!(image.rgba8_at(4, 4), [255, 255, 255, 255]);
}

#[test]
fn wgpu_renderer_isolates_opacity_layer_with_offscreen_child_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_opacity_layer(full.to_path(0.0), Affine::IDENTITY, 0.0, 0.5);
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 128, 0));
    canvas.push_filter_layer(
        Filter::Opacity(1.0),
        Region::rect(full, crate::Radius::ZERO),
    );
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 0, 255));
    canvas.pop_layer();
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(8, 8), [0, 0, 128, 128]);
}

#[test]
fn wgpu_renderer_isolates_blend_layer_with_offscreen_child_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(128, 128, 128));
    canvas.push_blend_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    canvas.push_filter_layer(
        Filter::Opacity(1.0),
        Region::rect(full, crate::Radius::ZERO),
    );
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(0, 255, 0));
    canvas.pop_layer();
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(4, 8), [0, 128, 0, 255]);
    assert_eq!(image.rgba8_at(12, 8), [128, 128, 128, 255]);
}

#[test]
fn wgpu_renderer_isolates_plain_layer_with_child_blend_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(128, 128, 128));
    canvas.push_isolate_layer(full.to_path(0.0), Affine::IDENTITY, 0.0);
    canvas.push_blend_layer(
        full.to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    canvas.pop_layer();
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(8, 8), [255, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_alpha_mask_layer_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut mask_scene = Canvas::new(16, 16, 1.0);
    mask_scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgba8(255, 255, 255, 128),
    );

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_mask_layer(
        mask_scene,
        Mask {
            region: Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
            kind: MaskKind::Alpha,
        },
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(4, 8), [128, 0, 0, 128]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_outer_clip_stack_to_offscreen_output_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_clip_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    canvas.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(4, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_outer_sdf_clip_stack_to_offscreen_output_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO);
    canvas.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(4, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_backdrop_filter_to_existing_target_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(48, 24, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 48.0, 24.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_backdrop_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(8.0, 4.0, 32.0, 20.0), crate::Radius::ZERO),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(12, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(4, 8), [255, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_renders_backdrop_layer_children_after_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(32, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_backdrop_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(4.0, 4.0, 12.0, 12.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(2, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
    assert_eq!(image.rgba8_at(24, 8), [255, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_offset_filter_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::Offset { dx: 2.0, dy: 1.0 },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 4.0, 4.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(1, 2), [0, 0, 0, 0]);
    assert_eq!(image.rgba8_at(3, 2), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(6, 2), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_blurs_offscreen_children_into_expanded_bounds_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let sample = Rect::new(24.0, 8.0, 40.0, 24.0);
    let mut canvas = Canvas::new(64, 32, 1.0);
    canvas.push_filter_layer(
        Filter::Blur {
            std_dev_x: 2.0,
            std_dev_y: 2.0,
            sampling: Default::default(),
        },
        Region::rect(sample, crate::Radius::ZERO),
    );
    canvas.push_rect(sample, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expanded = image.rgba8_at(22, 16);

    assert!(
        expanded[0] > 0 && expanded[3] > 0,
        "expected blur outside source rect, got {expanded:?}"
    );
    assert_eq!(image.rgba8_at(12, 16), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_downsampled_blur_is_stable_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let sample = Rect::new(7.0, 5.0, 39.0, 27.0);
    let mut canvas = Canvas::new(64, 40, 1.0);
    canvas.push_filter_layer(
        Filter::Blur {
            std_dev_x: 4.0,
            std_dev_y: 4.0,
            sampling: BlurSampling::downsampled(3),
        },
        Region::rect(sample, crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(10.0, 8.0, 24.0, 22.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_rect(
        Rect::new(22.0, 12.0, 36.0, 25.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 80, 255),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(&image, &expected, 0, "downsampled blur");
}

#[test]
fn wgpu_renderer_shared_blur_is_stable_across_workgroup_edges() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(73, 55, 1.0);
    canvas.push_filter_layer(
        Filter::Blur {
            std_dev_x: 5.0,
            std_dev_y: 5.0,
            sampling: BlurSampling::default(),
        },
        Region::rect(Rect::new(3.0, 4.0, 68.0, 51.0), crate::Radius::ZERO),
    );
    for x in (4..68).step_by(5) {
        let color = if x % 2 == 0 {
            Color::from_rgb8(255, 40, 80)
        } else {
            Color::from_rgb8(30, 140, 255)
        };
        canvas.push_rect(
            Rect::new(f64::from(x), 6.0, f64::from(x + 2), 49.0),
            crate::Radius::ZERO,
            color,
        );
    }
    for y in (7..51).step_by(7) {
        canvas.push_rect(
            Rect::new(5.0, f64::from(y), 66.0, f64::from(y + 2)),
            crate::Radius::ZERO,
            Color::from_rgba8(20, 220, 120, 180),
        );
    }
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(&image, &expected, 0, "shared blur workgroup edges");
}

#[test]
fn wgpu_renderer_global_blur_is_stable_with_paired_linear_samples() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(96, 72, 1.0);
    canvas.push_filter_layer(
        Filter::Blur {
            std_dev_x: 7.0,
            std_dev_y: 7.0,
            sampling: BlurSampling::default(),
        },
        Region::rect(Rect::new(5.0, 6.0, 90.0, 66.0), crate::Radius::ZERO),
    );
    for x in (7..90).step_by(8) {
        let color = if x % 3 == 0 {
            Color::from_rgb8(240, 32, 80)
        } else {
            Color::from_rgb8(20, 180, 255)
        };
        canvas.push_rect(
            Rect::new(f64::from(x), 9.0, f64::from(x + 3), 63.0),
            crate::Radius::ZERO,
            color,
        );
    }
    for y in (10..66).step_by(9) {
        canvas.push_rect(
            Rect::new(8.0, f64::from(y), 87.0, f64::from(y + 3)),
            crate::Radius::ZERO,
            Color::from_rgba8(255, 220, 40, 160),
        );
    }
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(&image, &expected, 0, "global paired blur");
}

#[test]
fn wgpu_renderer_profiles_filter_dispatch_stages_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let sample = Rect::new(7.0, 5.0, 39.0, 27.0);
    let mut canvas = Canvas::new(64, 40, 1.0);
    canvas.push_filter_layer(
        Filter::Blur {
            std_dev_x: 4.0,
            std_dev_y: 4.0,
            sampling: BlurSampling::downsampled(3),
        },
        Region::rect(sample, crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(10.0, 8.0, 24.0, 22.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let mut renderer = new_test_renderer(64, 40, Color::TRANSPARENT);
    let profile = renderer.render_profiled(&canvas);

    assert_profile_has(&profile, "filter.downsample");
    assert_profile_has(&profile, "filter.blur.x");
    assert_profile_has(&profile, "filter.blur.y");
    assert_profile_has(&profile, "filter.upsample");
    assert_profile_has(&profile, "filter.composite.surface.direct");
    assert_profile_missing(&profile, "filter.stack.surface");
}

#[test]
fn wgpu_renderer_profiles_empty_stack_backdrop_with_direct_composite_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(64, 40, 1.0);
    for x in 0..64 {
        let v = (x * 3) as u8;
        canvas.push_rect(
            Rect::new(f64::from(x), 0.0, f64::from(x + 1), 40.0),
            crate::Radius::ZERO,
            Color::from_rgb8(v, 90, 255u8.saturating_sub(v)),
        );
    }
    canvas.push_backdrop_layer(
        Filter::Blur {
            std_dev_x: 4.0,
            std_dev_y: 4.0,
            sampling: BlurSampling::downsampled(3),
        },
        Region::rect(Rect::new(12.0, 8.0, 52.0, 32.0), crate::Radius::all(6.0)),
    );
    canvas.pop_layer();

    let mut renderer = new_test_renderer(64, 40, Color::TRANSPARENT);
    let profile = renderer.render_profiled(&canvas);

    assert_profile_has(&profile, "filter.upsample.composite.rect");
    assert_profile_missing(&profile, "filter.upsample");
    assert_profile_missing(&profile, "filter.composite.rect_direct");
    assert_profile_missing(&profile, "filter.copy");
    assert_profile_missing(&profile, "filter.mask.rect");
    assert_profile_missing(&profile, "filter.composite.direct");
    assert_profile_missing(&profile, "filter.stack.src_over");
}

#[test]
fn wgpu_renderer_profiles_empty_stack_liquid_glass_with_direct_composite_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(64, 40, 1.0);
    for x in 0..64 {
        let v = (x * 3) as u8;
        canvas.push_rect(
            Rect::new(f64::from(x), 0.0, f64::from(x + 1), 40.0),
            crate::Radius::ZERO,
            Color::from_rgb8(v, 90, 255u8.saturating_sub(v)),
        );
    }
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 12,
            blur_sampling: BlurSampling::downsampled(4),
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(12.0, 8.0, 52.0, 32.0), crate::Radius::all(6.0)),
    );
    canvas.pop_layer();

    let mut renderer = new_test_renderer(64, 40, Color::TRANSPARENT);
    let profile = renderer.render_profiled(&canvas);

    assert_profile_has(&profile, "filter.copy");
    assert_profile_has(&profile, "filter.downsample");
    assert_profile_has(&profile, "filter.blur.x");
    assert_profile_has(&profile, "filter.blur.y");
    assert_profile_has(&profile, "filter.upsample");
    assert_profile_has(&profile, "filter.liquid_glass.composite.rect");
    assert_profile_missing(&profile, "filter.liquid_glass");
    assert_profile_missing(&profile, "filter.composite.rect_direct");
    assert_profile_missing(&profile, "filter.stack.src_over");
}

#[test]
fn persistent_full_redraw_liquid_glass_uses_direct_composite() {
    if !run_wgpu_tests() {
        return;
    }

    let mut background = Canvas::new(64, 40, 1.0);
    background.push_rect(
        Rect::new(0.0, 0.0, 64.0, 40.0),
        crate::Radius::ZERO,
        Color::from_rgb8(32, 96, 160),
    );
    background.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 12,
            blur_sampling: BlurSampling::downsampled(4),
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(12.0, 8.0, 52.0, 32.0), crate::Radius::all(6.0)),
    );
    background.pop_layer();

    let root = crate::RetainedNodeId::for_owner(68_200);
    let mut scene = RetainedScene::new(64, 40, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            crate::RetainedNodeId::for_owner(68_201),
            std::sync::Arc::new(background),
            Point::ZERO,
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(64, 40, Color::TRANSPARENT);
    let mut config = renderer.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    renderer.set_incremental_render_config(config);
    renderer.start_profile();
    renderer.render_retained(&scene);
    let profile = renderer.end_profile().clone();

    assert!(renderer.incremental_render_stats().full_redraw);
    assert_profile_has(&profile, "filter.liquid_glass.composite.rect");
    assert_profile_missing(&profile, "filter.liquid_glass");
    assert_profile_missing(&profile, "filter.composite.rect_direct");
    assert_profile_missing(&profile, "filter.stack.src_over");
}

#[test]
fn persistent_full_redraw_clipped_liquid_glass_skips_unused_source_history() {
    if !run_wgpu_tests() {
        return;
    }

    let mut scene = Canvas::new(64, 40, 1.0);
    scene.push_rect(
        Rect::new(0.0, 0.0, 64.0, 40.0),
        crate::Radius::ZERO,
        Color::from_rgb8(32, 96, 160),
    );
    scene.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 64.0, 40.0), crate::Radius::all(4.0));
    scene.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 12,
            blur_sampling: BlurSampling::downsampled(4),
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(12.0, 8.0, 52.0, 32.0), crate::Radius::all(6.0)),
    );
    scene.pop_layer();
    scene.pop_layer();
    let immediate = scene.clone();

    let root = crate::RetainedNodeId::for_owner(68_210);
    let mut retained = RetainedScene::new(64, 40, 1.0, root).unwrap();
    retained
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            crate::RetainedNodeId::for_owner(68_211),
            std::sync::Arc::new(scene),
            Point::ZERO,
        )
        .commit()
        .unwrap();
    let mut renderer = new_test_renderer(64, 40, Color::TRANSPARENT);
    let mut config = renderer.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    renderer.set_incremental_render_config(config);
    renderer.start_profile();
    renderer.render_retained(&retained);
    let profile = renderer.end_profile().clone();
    let mut immediate_renderer = new_test_renderer(64, 40, Color::TRANSPARENT);
    let immediate_profile = immediate_renderer.render_profiled(&immediate);

    assert!(renderer.incremental_render_stats().full_redraw);
    assert_profile_has(&profile, "filter.liquid_glass");
    assert_eq!(
        profile
            .entries()
            .iter()
            .filter(|entry| entry.name == "filter.copy")
            .count(),
        immediate_profile
            .entries()
            .iter()
            .filter(|entry| entry.name == "filter.copy")
            .count(),
        "full retained redraw must not add a copy for unused source history"
    );
}

#[test]
fn wgpu_renderer_profiles_simple_liquid_glass_without_materialized_upsample_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(64, 40, 1.0);
    for x in 0..64 {
        let v = (x * 3) as u8;
        canvas.push_rect(
            Rect::new(f64::from(x), 0.0, f64::from(x + 1), 40.0),
            crate::Radius::ZERO,
            Color::from_rgb8(v, 90, 255u8.saturating_sub(v)),
        );
    }
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 12,
            blur_sampling: BlurSampling::downsampled(4),
            refraction_dispersion: 0.0,
            fresnel_factor: 0.0,
            glare_factor: 0.0,
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(12.0, 8.0, 52.0, 32.0), crate::Radius::all(6.0)),
    );
    canvas.pop_layer();

    let mut renderer = new_test_renderer(64, 40, Color::TRANSPARENT);
    let profile = renderer.render_profiled(&canvas);

    assert_profile_has(&profile, "filter.copy");
    assert_profile_has(&profile, "filter.downsample");
    assert_profile_has(&profile, "filter.blur.x");
    assert_profile_has(&profile, "filter.blur.y");
    assert_profile_has(&profile, "filter.liquid_glass.composite.rect");
    assert_profile_missing(&profile, "filter.upsample");
    assert_profile_missing(&profile, "filter.liquid_glass");
    assert_profile_missing(&profile, "filter.composite.rect_direct");
    assert_profile_missing(&profile, "filter.stack.src_over");
}

#[test]
fn wgpu_renderer_profiles_liquid_glass_repeated_gpu_timestamps_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(64, 40, 1.0);
    for x in 0..64 {
        let v = (x * 3) as u8;
        canvas.push_rect(
            Rect::new(f64::from(x), 0.0, f64::from(x + 1), 40.0),
            crate::Radius::ZERO,
            Color::from_rgb8(v, 90, 255u8.saturating_sub(v)),
        );
    }
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 12,
            blur_sampling: BlurSampling::downsampled(4),
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(12.0, 8.0, 52.0, 32.0), crate::Radius::all(6.0)),
    );
    canvas.pop_layer();

    let mut renderer = new_test_renderer(64, 40, Color::TRANSPARENT);
    if !renderer
        .device()
        .features()
        .contains(::wgpu::Features::TIMESTAMP_QUERY)
    {
        return;
    }

    for _ in 0..8 {
        renderer.start_profile();
        renderer.render(&canvas);
        let _ = renderer.end_profile();
        renderer
            .device()
            .poll(::wgpu::PollType::wait_indefinitely())
            .expect("poll wgpu device for liquid glass profile readback");
        let profile = renderer.poll_profile().clone();
        assert!(profile.entries().iter().any(|entry| {
            entry.name == "filter.liquid_glass.composite.rect" && entry.gpu_duration.is_some()
        }));
    }
}

#[test]
fn wgpu_renderer_applies_morphology_filter_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::Morphology {
            radius_x: 1.0,
            radius_y: 1.0,
            operator: MorphologyOperator::Dilate,
        },
        Region::rect(Rect::new(4.0, 4.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(4.0, 4.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(3, 5), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(5, 5), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(2, 5), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_spills_deep_clip_stack_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(32, 16, 1.0);
    let depth = crate::shared::gpu_plan::FINE_LOCAL_CLIP_DEPTH + 2;
    for ix in 0..depth {
        canvas.push_clip_layer(
            Rect::new(ix as f64 * 4.0, 0.0, 32.0, 16.0).to_path(0.0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
    }
    canvas.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    for _ in 0..depth {
        canvas.pop_layer();
    }

    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    renderer.render(&canvas);

    let expected = render_native_wgpu(&canvas);
    assert_images_near(&renderer.image(), &expected, 0, "deep clip spill");
}

#[test]
fn wgpu_renderer_spills_deep_opacity_stack_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    let depth = crate::shared::gpu_plan::FINE_LOCAL_GROUP_DEPTH + 2;
    for _ in 0..depth {
        canvas.push_opacity_layer(full.to_path(0.0), Affine::IDENTITY, 0.0, 0.5);
    }
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(255, 0, 0));
    for _ in 0..depth {
        canvas.pop_layer();
    }

    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    renderer.render(&canvas);

    let expected = render_native_wgpu(&canvas);
    assert_images_near(&renderer.image(), &expected, 0, "deep opacity spill");
}

#[test]
fn wgpu_renderer_draws_text_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut canvas = Canvas::new(160, 64, 1.0);
    canvas.push_text_layout(&layout, peniko::kurbo::Point::new(8.0, 32.0), Color::BLACK);
    let mut renderer = new_test_renderer(160, 64, Color::TRANSPARENT);

    renderer.render_with_text(&canvas, &mut font_system, &mut text_context);
    let image = renderer.image();

    assert!(
        image.pixels.iter().any(|pixel| (pixel >> 24) != 0),
        "expected at least one text pixel"
    );
}

#[test]
fn wgpu_renderer_ignores_glyph_runs_without_prepared_text_data() {
    if !run_wgpu_tests() {
        return;
    }

    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut canvas = Canvas::new(160, 64, 1.0);
    canvas.push_text_layout(&layout, peniko::kurbo::Point::new(8.0, 32.0), Color::BLACK);
    let mut renderer = new_test_renderer(160, 64, Color::TRANSPARENT);

    renderer.render(&canvas);

    assert!(
        renderer
            .image()
            .pixels
            .iter()
            .all(|pixel| (pixel >> 24) == 0),
        "unprepared glyph runs should not sample empty text buffers"
    );
}

#[test]
fn wgpu_renderer_renders_text_directly_to_storage_texture_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut canvas = Canvas::new(160, 64, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 160.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(236, 238, 242),
    );
    canvas.push_text_layout(
        &layout,
        peniko::kurbo::Point::new(8.0, 36.0),
        Color::from_rgb8(18, 24, 36),
    );

    let mut renderer = new_test_renderer(160, 64, Color::TRANSPARENT);
    if !renderer
        .device()
        .features()
        .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        return;
    }
    let texture = renderer
        .device()
        .create_texture(&::wgpu::TextureDescriptor {
            label: Some("tileink wgpu renderer direct text storage texture test"),
            size: ::wgpu::Extent3d {
                width: 160,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ::wgpu::TextureDimension::D2,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            usage: ::wgpu::TextureUsages::STORAGE_BINDING | ::wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

    renderer
        .render_with_text_to_wgpu_texture(&canvas, &mut font_system, &mut text_context, &texture)
        .expect("render text directly to wgpu storage texture");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 160, 64);

    assert!(
        bytes.chunks_exact(4).any(|px| px[3] != 0),
        "expected direct text texture to contain non-transparent pixels"
    );
}

#[test]
fn wgpu_renderer_renders_text_compositing_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let layout = text_context.layout(&mut font_system, TextLayoutOptions::new("Text", 28.0));
    if layout.is_empty() {
        return;
    }
    let mut canvas = Canvas::new(160, 64, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 160.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(236, 238, 242),
    );
    canvas.push_text_layout(
        &layout,
        peniko::kurbo::Point::new(8.0, 36.0),
        Color::from_rgb8(18, 24, 36),
    );

    let mut renderer = new_test_renderer(160, 64, Color::TRANSPARENT);
    renderer.render_with_text(&canvas, &mut font_system, &mut text_context);
    let wgpu_image = renderer.image();

    assert!(
        (0..wgpu_image.height).any(|y| {
            (0..wgpu_image.width).any(|x| wgpu_image.rgba8_at(x, y)[..3] != [236, 238, 242])
        }),
        "expected text compositing to modify the background"
    );
}

fn run_wgpu_tests() -> bool {
    std::env::var("TILEINK_RUN_WGPU_TESTS").as_deref() == Ok("1")
        || std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() == Ok("1")
}

fn new_test_renderer(width: u32, height: u32, clear: Color) -> Renderer {
    let portable = std::env::var("TILEINK_WGPU_MODE").as_deref() == Ok("portable");
    let Some((device, queue)) = shared_wgpu_test_device(portable) else {
        return Renderer::new_default_device(width, height, clear);
    };
    Renderer::new(device, queue, width, height, clear)
}

fn shared_wgpu_test_device(portable: bool) -> Option<&'static (::wgpu::Device, ::wgpu::Queue)> {
    use std::sync::OnceLock;

    static NATIVE: OnceLock<Option<(::wgpu::Device, ::wgpu::Queue)>> = OnceLock::new();
    static PORTABLE: OnceLock<Option<(::wgpu::Device, ::wgpu::Queue)>> = OnceLock::new();
    let slot = if portable { &PORTABLE } else { &NATIVE };
    slot.get_or_init(|| {
        let instance =
            ::wgpu::Instance::new(::wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&::wgpu::RequestAdapterOptions {
                power_preference: ::wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .ok()?;
        let optional_native = ::wgpu::Features::TIMESTAMP_QUERY
            | ::wgpu::Features::TEXTURE_BINDING_ARRAY
            | ::wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
        pollster::block_on(adapter.request_device(&::wgpu::DeviceDescriptor {
            label: Some(if portable {
                "tileink portable shared test device"
            } else {
                "tileink native shared test device"
            }),
            required_features: if portable {
                ::wgpu::Features::empty()
            } else {
                adapter.features() & optional_native
            },
            required_limits: adapter.limits(),
            memory_hints: ::wgpu::MemoryHints::Performance,
            trace: ::wgpu::Trace::Off,
            experimental_features: ::wgpu::ExperimentalFeatures::disabled(),
        }))
        .ok()
    })
    .as_ref()
}

fn test_turbulence(kind: TurbulenceKind, seed: i32, num_octaves: u32) -> Turbulence {
    Turbulence {
        base_frequency_x: 0.07,
        base_frequency_y: 0.11,
        num_octaves,
        seed,
        stitch_tiles: false,
        kind,
        linear_rgb: false,
        transform_x: 0.0,
        transform_y: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        tile_x: 0.0,
        tile_y: 0.0,
        tile_width: 32.0,
        tile_height: 24.0,
    }
}

fn render_native_wgpu(canvas: &Canvas) -> crate::shared::image::Image {
    let mut renderer = Renderer::new_default_device(
        canvas.physical_width(),
        canvas.physical_height(),
        Color::TRANSPARENT,
    );
    renderer.prepare_scene(canvas);
    assert!(
        renderer.render_prepared_tile_plan(canvas),
        "expected canvas to render through native wgpu path"
    );
    renderer.image()
}

fn assert_images_near(
    actual: &crate::shared::image::Image,
    expected: &crate::shared::image::Image,
    tolerance: u8,
    context: &str,
) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    for y in 0..actual.height {
        for x in 0..actual.width {
            let a = actual.rgba8_at(x, y);
            let e = expected.rgba8_at(x, y);
            for channel in 0..4 {
                let diff = a[channel].abs_diff(e[channel]);
                assert!(
                    diff <= tolerance,
                    "{context} mismatch at ({x}, {y}) channel {channel}: actual {a:?}, expected {e:?}"
                );
            }
        }
    }
}

fn assert_profile_has(profile: &crate::WgpuRenderProfile, name: &'static str) {
    assert!(
        profile.entries().iter().any(|entry| entry.name == name),
        "profile missing {name}; entries: {:?}",
        profile.entries()
    );
}

fn assert_profile_missing(profile: &crate::WgpuRenderProfile, name: &'static str) {
    assert!(
        !profile.entries().iter().any(|entry| entry.name == name),
        "profile unexpectedly included {name}; entries: {:?}",
        profile.entries()
    );
}

fn initialized_compute_pipeline_counts(renderer: &Renderer) -> [usize; 4] {
    [
        renderer
            .scan_pipeline
            .as_ref()
            .map_or(0, |pipeline| pipeline.initialized_pipeline_count()),
        renderer
            .cumsum
            .as_ref()
            .map_or(0, |pipeline| pipeline.initialized_pipeline_count()),
        renderer
            .coarse_pipeline
            .as_ref()
            .map_or(0, |pipeline| pipeline.initialized_pipeline_count()),
        renderer
            .fine
            .as_ref()
            .map_or(0, |pipeline| pipeline.initialized_pipeline_count()),
    ]
}

fn read_render_target_u32(renderer: &Renderer, target: WgpuRenderTargetId, len: usize) -> Vec<u32> {
    let texture = match target {
        WgpuRenderTargetId::Main => renderer.readback_target.texture(),
        WgpuRenderTargetId::Scratch(ix) => renderer.scratch[ix].texture(),
    };
    let bytes = read_texture_rgba8(
        renderer.device(),
        renderer.queue(),
        texture,
        renderer.size.0,
        renderer.size.1,
    );
    bytemuck::cast_slice(&bytes)[..len].to_vec()
}

/// Runs the first compiled draw batch with its fused layer stack.
///
/// Clip commands are structural execution-plan entries, not ordinary draws in
/// the child batch. Coarse tests must follow the compiled contract used by the
/// renderer or they silently omit the clip wrappers they intend to inspect.
fn coarse_first_draw_batch(renderer: &mut Renderer, canvas: &Canvas) {
    let (batch_id, layer_stack) = renderer
        .plan
        .as_ref()
        .expect("prepared execution plan")
        .ops
        .iter()
        .find_map(|op| match op {
            ExecOp::DrawBatch {
                batch_id,
                layer_stack,
                ..
            } => Some((*batch_id, layer_stack.clone())),
            _ => None,
        })
        .expect("execution plan contains a draw batch");
    // Path clips in the fused stack consume scan/cumsum backdrops before
    // coarse can decide whether the clip is a no-op or needs wrapper particles.
    renderer.scan_for_test();
    renderer.cumsum_for_test();
    renderer.coarse_batch(
        canvas,
        batch_id,
        batch_id + 1,
        layer_stack.start as u32,
        layer_stack.end as u32,
    );
}

fn read_ptcl_tags(renderer: &Renderer, len: usize) -> Vec<u32> {
    read_ptcl_records(renderer, len)
        .into_iter()
        .map(|record| record.tag)
        .collect()
}

fn read_ptcl_colors(renderer: &Renderer, len: usize) -> Vec<u32> {
    read_ptcl_records(renderer, len)
        .into_iter()
        .map(|record| record.color)
        .collect()
}

fn read_ptcl_records(renderer: &Renderer, len: usize) -> Vec<PtclRecord> {
    renderer.coarse.read_ptcl_records(
        renderer.device(),
        renderer.queue(),
        renderer.lengths.tile_count,
        len,
    )
}

fn create_test_target_texture(
    renderer: &Renderer,
    width: u32,
    height: u32,
    label: &str,
) -> ::wgpu::Texture {
    renderer
        .device()
        .create_texture(&::wgpu::TextureDescriptor {
            label: Some(label),
            size: ::wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ::wgpu::TextureDimension::D2,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            usage: ::wgpu::TextureUsages::STORAGE_BINDING
                | ::wgpu::TextureUsages::COPY_DST
                | ::wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
}

fn read_texture_rgba8(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    texture: &::wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let row_bytes = width as ::wgpu::BufferAddress * 4;
    let padded_row_bytes = row_bytes.next_multiple_of(::wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64);
    let readback = device.create_buffer(&::wgpu::BufferDescriptor {
        label: Some("tileink wgpu renderer texture readback"),
        size: padded_row_bytes * height as u64,
        usage: ::wgpu::BufferUsages::COPY_DST | ::wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&::wgpu::CommandEncoderDescriptor {
        label: Some("tileink wgpu renderer texture readback copy"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        ::wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: ::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: (height > 1).then_some(padded_row_bytes as u32),
                rows_per_image: None,
            },
        },
        ::wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(::wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap()
        });
    device.poll(::wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();

    let view = readback
        .slice(..)
        .get_mapped_range()
        .expect("read mapped wgpu test readback buffer");
    let mut tight = Vec::with_capacity((row_bytes * height as u64) as usize);
    for row in 0..height as usize {
        let start = row * padded_row_bytes as usize;
        tight.extend_from_slice(&view[start..start + row_bytes as usize]);
    }
    drop(view);
    readback.unmap();
    tight
}
