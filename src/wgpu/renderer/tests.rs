use peniko::{
    Color, Compose, Extend, Gradient, Mix,
    kurbo::{Affine, BezPath, Line, Rect, Shape},
};

use super::{Renderer, RendererOptions, WgpuRenderTargetId};
use crate::wgpu::coarse::force_coarse_emit_chunks_for_test;
use crate::wgpu::commands::WgpuCommandBatch;
use crate::{
    Canvas, FillRule, Image, ImageKey, PatternSampling, RetainedLayerKey, RetainedNodeId,
    TextContext, TextFontSystem, TextLayoutOptions,
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
fn retained_renderer_updates_only_changed_tiles_and_matches_full_render() {
    if !run_wgpu_tests() {
        return;
    }

    fn child(color: Color) -> std::sync::Arc<Canvas> {
        let mut scene = Canvas::new(16, 16, 1.0);
        scene.push_rect(Rect::new(1.0, 1.0, 15.0, 15.0), crate::Radius::ZERO, color);
        std::sync::Arc::new(scene)
    }

    let root = RetainedNodeId::for_owner(1);
    let left = RetainedNodeId::for_owner(2);
    let right = RetainedNodeId::for_owner(3);
    let blue = child(Color::from_rgb8(20, 40, 220));
    let mut first = Canvas::new_retained(64, 32, 1.0, root);
    first.append_retained_scene(left, 0, child(Color::from_rgb8(220, 40, 20)), (0.0, 0.0));
    first.append_retained_scene(right, 0, blue.clone(), (32.0, 0.0));

    let mut second = Canvas::new_retained(64, 32, 1.0, root);
    second.append_retained_scene(left, 1, child(Color::from_rgb8(20, 220, 40)), (0.0, 0.0));
    second.append_retained_scene(right, 0, blue, (32.0, 0.0));

    let mut incremental = new_test_renderer(64, 32, Color::TRANSPARENT);
    incremental.render(&first);
    let first_image = incremental.image();
    incremental.render(&second);
    let incremental_image = incremental.image();
    assert!(!incremental.incremental_render_stats().full_redraw);
    assert_eq!(incremental.incremental_render_stats().dirty_tiles, 1);
    assert!(incremental.incremental_render_stats().reused_compiled_plan);
    assert_eq!(incremental.incremental_render_stats().draw_batches, 1);
    assert_eq!(incremental.incremental_render_stats().root_draw_batches, 1);
    assert_eq!(incremental.incremental_render_stats().filter_dispatches, 1);
    assert_eq!(
        incremental
            .incremental_render_stats()
            .compact_filter_dispatches,
        1
    );

    let mut full = new_test_renderer(64, 32, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&second);
    let full_image = full.image();
    assert_eq!(incremental_image.width, full_image.width);
    assert_eq!(incremental_image.height, full_image.height);
    assert_eq!(incremental_image.pixels, full_image.pixels);
    assert_eq!(incremental_image.rgba8_at(8, 8), [20, 220, 40, 255]);
    assert_eq!(incremental_image.rgba8_at(40, 8), [20, 40, 220, 255]);
    for y in 0..32 {
        for x in 32..64 {
            assert_eq!(
                incremental_image.rgba8_at(x, y),
                first_image.rgba8_at(x, y),
                "clean tile changed at ({x}, {y})"
            );
        }
    }
}

#[test]
fn retained_components_keep_only_cached_scene_nodes_and_match_full_render() {
    if !run_wgpu_tests() {
        return;
    }

    fn frame(left_revision: u64, left_color: Color) -> Canvas {
        let mut root = Canvas::new_retained(64, 32, 1.0, RetainedNodeId::for_owner(400));
        for (owner, revision, x, color) in [
            (401, left_revision, 0.0, left_color),
            (402, 0, 32.0, Color::from_rgb8(20, 40, 220)),
        ] {
            let mut child = Canvas::new(16, 16, 1.0);
            child.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
            root.append_retained_scene(
                RetainedNodeId::new(owner, 1),
                revision,
                std::sync::Arc::new(child),
                (x, 0.0),
            );
        }
        root
    }

    let first = frame(0, Color::from_rgb8(220, 40, 20));
    let second = frame(1, Color::from_rgb8(20, 220, 40));
    let mut incremental = new_test_renderer(64, 32, Color::TRANSPARENT);
    incremental.render(&first);
    incremental.render(&second);
    let stats = incremental.incremental_render_stats();
    assert_eq!(
        stats.retained_nodes, 2,
        "cached drawables must not gain generic component nodes"
    );
    assert_eq!(stats.dirty_tiles, 1);
    assert_eq!(stats.root_draw_batches, 1);

    let mut full = new_test_renderer(64, 32, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&second);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn retained_sparse_damage_clears_with_one_compact_dispatch() {
    if !run_wgpu_tests() {
        return;
    }

    fn child(color: Color) -> std::sync::Arc<Canvas> {
        let mut scene = Canvas::new(16, 16, 1.0);
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        std::sync::Arc::new(scene)
    }

    let root = RetainedNodeId::for_owner(10);
    let first_id = RetainedNodeId::for_owner(11);
    let second_id = RetainedNodeId::for_owner(12);
    let stable_id = RetainedNodeId::for_owner(13);
    let stable = child(Color::from_rgb8(20, 40, 220));

    let mut first = Canvas::new_retained(96, 64, 1.0, root);
    first.append_retained_scene(
        first_id,
        0,
        child(Color::from_rgb8(220, 20, 20)),
        (0.0, 0.0),
    );
    first.append_retained_scene(stable_id, 0, stable.clone(), (32.0, 16.0));
    first.append_retained_scene(
        second_id,
        0,
        child(Color::from_rgb8(20, 220, 20)),
        (80.0, 48.0),
    );

    let mut second = Canvas::new_retained(96, 64, 1.0, root);
    second.append_retained_scene(
        first_id,
        1,
        child(Color::from_rgb8(220, 220, 20)),
        (0.0, 0.0),
    );
    second.append_retained_scene(stable_id, 0, stable, (32.0, 16.0));
    second.append_retained_scene(
        second_id,
        1,
        child(Color::from_rgb8(20, 220, 220)),
        (80.0, 48.0),
    );

    let mut incremental = new_test_renderer(96, 64, Color::TRANSPARENT);
    incremental.render(&first);
    let stable_before = incremental.image().rgba8_at(40, 24);
    incremental.render(&second);
    let stats = incremental.incremental_render_stats();
    assert_eq!(stats.dirty_tiles, 2);
    assert_eq!(stats.filter_dispatches, 1);
    assert_eq!(stats.compact_filter_dispatches, 1);
    assert_eq!(incremental.image().rgba8_at(40, 24), stable_before);

    let mut full = new_test_renderer(96, 64, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&second);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn retained_path_scan_dispatches_only_paths_reaching_dirty_tiles() {
    if !run_wgpu_tests() {
        return;
    }

    fn path_scene(color: Color) -> std::sync::Arc<Canvas> {
        let mut path = BezPath::new();
        path.move_to((1.0, 1.0));
        path.line_to((15.0, 2.0));
        path.line_to((8.0, 15.0));
        path.close_path();
        let mut scene = Canvas::new(16, 16, 1.0);
        scene.push_path(path, color, Affine::IDENTITY, FillRule::NonZero, 0.0);
        std::sync::Arc::new(scene)
    }

    let root = RetainedNodeId::for_owner(70);
    let left = RetainedNodeId::for_owner(71);
    let right = RetainedNodeId::for_owner(72);
    let right_scene = path_scene(Color::from_rgb8(20, 40, 220));
    let mut first = Canvas::new_retained(64, 16, 1.0, root);
    first.append_retained_scene(
        left,
        0,
        path_scene(Color::from_rgb8(220, 40, 20)),
        (0.0, 0.0),
    );
    first.append_retained_scene(right, 0, right_scene.clone(), (48.0, 0.0));
    let mut second = Canvas::new_retained(64, 16, 1.0, root);
    second.append_retained_scene(
        left,
        1,
        path_scene(Color::from_rgb8(20, 220, 40)),
        (0.0, 0.0),
    );
    second.append_retained_scene(right, 0, right_scene, (48.0, 0.0));
    let mut incremental = new_test_renderer(64, 16, Color::TRANSPARENT);

    incremental.render(&first);
    incremental.render(&second);

    assert!(!incremental.incremental_render_stats().full_redraw);
    assert_eq!(incremental.incremental_render_stats().dirty_tiles, 1);
    assert_eq!(incremental.incremental_render_stats().scanned_paths, 1);
    let mut full = new_test_renderer(64, 16, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&second);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn retained_scene_revision_change_and_removal_dirty_without_manual_damage() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(80);
    let node = RetainedNodeId::for_owner(81);
    let frame = |revision, color: Option<Color>| {
        let mut canvas = Canvas::new_retained(32, 16, 1.0, root);
        if let Some(color) = color {
            let mut child = Canvas::new(16, 16, 1.0);
            child.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
            canvas.append_retained_scene(node, revision, std::sync::Arc::new(child), (0.0, 0.0));
        }
        canvas
    };
    let red = frame(0, Some(Color::from_rgb8(220, 30, 40)));
    let green = frame(1, Some(Color::from_rgb8(30, 210, 70)));
    let empty = frame(1, None);
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);

    renderer.render(&red);
    renderer.render(&green);
    assert!(!renderer.incremental_render_stats().full_redraw);
    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 1);
    assert_eq!(renderer.image().rgba8_at(8, 8), [30, 210, 70, 255]);

    renderer.render(&empty);
    assert!(!renderer.incremental_render_stats().full_redraw);
    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 1);
    assert_eq!(renderer.image().rgba8_at(8, 8), [0, 0, 0, 0]);
}

#[test]
fn untracked_previous_frame_is_not_committed_as_incremental_history() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(82);
    let child_id = RetainedNodeId::for_owner(83);
    let child = std::sync::Arc::new(Canvas::new(32, 16, 1.0));
    let mut first = Canvas::new_retained(32, 16, 1.0, root);
    first.append_retained_scene(child_id, 0, child.clone(), (0.0, 0.0));
    first.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(220, 30, 40),
    );
    let mut second = Canvas::new_retained(32, 16, 1.0, root);
    second.append_retained_scene(child_id, 0, child, (0.0, 0.0));
    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);

    renderer.render(&first);
    renderer.render(&second);

    assert_eq!(
        renderer.incremental_render_stats().full_redraw_reason,
        Some(crate::FullRedrawReason::UntrackedContent)
    );
    assert_eq!(renderer.image().rgba8_at(8, 8), [0, 0, 0, 0]);
}

#[test]
fn retained_renderer_copies_complete_history_to_external_texture() {
    if !run_wgpu_tests() {
        return;
    }

    fn scene(color: Color) -> std::sync::Arc<Canvas> {
        let mut scene = Canvas::new(16, 16, 1.0);
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        std::sync::Arc::new(scene)
    }

    let root = RetainedNodeId::for_owner(5);
    let mut first = Canvas::new_retained(32, 16, 1.0, root);
    first.append_retained_scene(
        RetainedNodeId::for_owner(6),
        0,
        scene(Color::from_rgb8(220, 30, 40)),
        (0.0, 0.0),
    );
    first.append_retained_scene(
        RetainedNodeId::for_owner(7),
        0,
        scene(Color::from_rgb8(20, 50, 220)),
        (16.0, 0.0),
    );
    let mut second = Canvas::new_retained(32, 16, 1.0, root);
    second.append_retained_scene(
        RetainedNodeId::for_owner(6),
        1,
        scene(Color::from_rgb8(30, 210, 70)),
        (0.0, 0.0),
    );
    second.append_retained_scene(
        RetainedNodeId::for_owner(7),
        0,
        scene(Color::from_rgb8(20, 50, 220)),
        (16.0, 0.0),
    );

    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    let texture = renderer
        .device()
        .create_texture(&::wgpu::TextureDescriptor {
            label: Some("tileink retained external target test"),
            size: ::wgpu::Extent3d {
                width: 32,
                height: 16,
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
        });
    renderer
        .render_to_wgpu_texture(&first, &texture)
        .expect("render first retained frame");
    renderer
        .render_to_wgpu_texture(&second, &texture)
        .expect("render second retained frame");
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 32, 16);
    assert_eq!(&bytes[4 * 8..4 * 9], &[30, 210, 70, 255]);
    assert_eq!(&bytes[4 * 24..4 * 25], &[20, 50, 220, 255]);
    let stats = renderer.incremental_render_stats();
    assert_eq!(stats.dirty_tiles, 1);
    assert_eq!(stats.queue_submissions, 1);
}

#[test]
fn high_damage_renders_directly_then_rebuilds_internal_history() {
    if !run_wgpu_tests() {
        return;
    }

    fn frame(revision: u64, color: Color) -> Canvas {
        let mut child = Canvas::new(48, 16, 1.0);
        child.push_rect(Rect::new(0.0, 0.0, 48.0, 16.0), crate::Radius::ZERO, color);
        let mut root = Canvas::new_retained(64, 16, 1.0, RetainedNodeId::for_owner(200));
        root.append_retained_scene(
            RetainedNodeId::for_owner(201),
            revision,
            std::sync::Arc::new(child),
            (0.0, 0.0),
        );
        root
    }

    let first = frame(0, Color::from_rgb8(220, 30, 40));
    let second = frame(1, Color::from_rgb8(30, 210, 70));
    let mut renderer = new_test_renderer(64, 16, Color::TRANSPARENT);
    let texture = create_test_target_texture(&renderer, 64, 16, "direct output state test");

    renderer
        .render_to_wgpu_texture(&first, &texture)
        .expect("build first retained history");
    assert_eq!(
        renderer.incremental_render_stats().output_mode,
        crate::IncrementalOutputMode::InternalHistory
    );
    assert!(renderer.incremental_render_stats().history_copied_to_output);
    assert_eq!(renderer.incremental_render_stats().queue_submissions, 1);

    renderer
        .render_to_wgpu_texture(&second, &texture)
        .expect("render high-damage frame directly");
    let stats = renderer.incremental_render_stats();
    assert_eq!(
        stats.full_redraw_reason,
        Some(crate::FullRedrawReason::DirtyTileThreshold)
    );
    assert_eq!(
        stats.output_mode,
        crate::IncrementalOutputMode::DirectTransient
    );
    assert!(!stats.history_copied_to_output);
    assert_eq!(
        &read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 64, 16)[4 * 8..4 * 9],
        &[30, 210, 70, 255]
    );

    renderer
        .render_to_wgpu_texture(&second, &texture)
        .expect("keep direct mode through first low-damage frame");
    assert_eq!(
        renderer.incremental_render_stats().output_mode,
        crate::IncrementalOutputMode::DirectTransient
    );
    assert_eq!(renderer.incremental_render_stats().changed_tiles, 0);
    assert_eq!(
        renderer.incremental_render_stats().dirty_tiles,
        renderer.incremental_render_stats().total_tiles
    );

    renderer
        .render_to_wgpu_texture(&second, &texture)
        .expect("rebuild history after hysteresis");
    assert_eq!(
        renderer.incremental_render_stats().output_mode,
        crate::IncrementalOutputMode::RebuildHistory
    );
    assert!(renderer.incremental_render_stats().history_copied_to_output);
    assert_eq!(renderer.incremental_render_stats().queue_submissions, 1);
}

#[test]
fn retained_resize_skips_internal_history_resize_and_copy() {
    if !run_wgpu_tests() {
        return;
    }

    fn frame(width: u32) -> Canvas {
        let mut child = Canvas::new(width, 16, 1.0);
        child.push_rect(
            Rect::new(0.0, 0.0, width as f64, 16.0),
            crate::Radius::ZERO,
            Color::from_rgb8(40, 120, 220),
        );
        let mut root = Canvas::new_retained(width, 16, 1.0, RetainedNodeId::for_owner(205));
        root.append_retained_scene(
            RetainedNodeId::for_owner(206),
            width as u64,
            std::sync::Arc::new(child),
            (0.0, 0.0),
        );
        root
    }

    let mut renderer = new_test_renderer(64, 16, Color::TRANSPARENT);
    let texture = create_test_target_texture(&renderer, 64, 16, "resize direct output test");
    renderer
        .render_to_wgpu_texture(&frame(32), &texture)
        .expect("initialize smaller history");
    assert_eq!(renderer.readback_target.size(), (32, 16));

    renderer
        .render_to_wgpu_texture(&frame(64), &texture)
        .expect("render resized frame directly");
    let stats = renderer.incremental_render_stats();
    assert_eq!(
        stats.full_redraw_reason,
        Some(crate::FullRedrawReason::SurfaceChanged)
    );
    assert_eq!(
        stats.output_mode,
        crate::IncrementalOutputMode::DirectTransient
    );
    assert!(!stats.history_copied_to_output);
    assert_eq!(renderer.readback_target.size(), (32, 16));
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 64, 16);
    assert_eq!(&bytes[4 * 48..4 * 49], &[40, 120, 220, 255]);

    renderer
        .render_to_wgpu_texture(&frame(64), &texture)
        .expect("remain direct for first stable frame");
    assert_eq!(renderer.readback_target.size(), (32, 16));
    renderer
        .render_to_wgpu_texture(&frame(64), &texture)
        .expect("rebuild resized history");
    assert_eq!(
        renderer.incremental_render_stats().output_mode,
        crate::IncrementalOutputMode::RebuildHistory
    );
    assert_eq!(renderer.readback_target.size(), (64, 16));
}

#[test]
fn persistent_external_texture_is_updated_in_place_without_history_copy() {
    if !run_wgpu_tests() {
        return;
    }

    fn child(color: Color) -> std::sync::Arc<Canvas> {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        std::sync::Arc::new(canvas)
    }

    let root_id = RetainedNodeId::for_owner(210);
    let left_id = RetainedNodeId::for_owner(211);
    let right_id = RetainedNodeId::for_owner(212);
    let blue = child(Color::from_rgb8(20, 50, 220));
    let mut first = Canvas::new_retained(32, 16, 1.0, root_id);
    first.append_retained_scene(left_id, 0, child(Color::from_rgb8(220, 30, 40)), (0.0, 0.0));
    first.append_retained_scene(right_id, 0, blue.clone(), (16.0, 0.0));
    let mut second = Canvas::new_retained(32, 16, 1.0, root_id);
    second.append_retained_scene(left_id, 1, child(Color::from_rgb8(30, 210, 70)), (0.0, 0.0));
    second.append_retained_scene(right_id, 0, blue, (16.0, 0.0));

    let mut renderer = new_test_renderer(32, 16, Color::TRANSPARENT);
    let texture = create_test_target_texture(&renderer, 32, 16, "persistent history test");
    let history_id = crate::ExternalTextureHistoryId::new(1);
    renderer
        .render_to_persistent_wgpu_texture(&first, &texture, history_id)
        .expect("initialize external history");
    renderer
        .render_to_persistent_wgpu_texture(&second, &texture, history_id)
        .expect("incrementally update external history");

    let stats = renderer.incremental_render_stats();
    assert_eq!(
        stats.output_mode,
        crate::IncrementalOutputMode::ExternalHistory
    );
    assert!(!stats.full_redraw);
    assert_eq!(stats.dirty_tiles, 1);
    assert!(!stats.history_copied_to_output);
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &texture, 32, 16);
    assert_eq!(&bytes[4 * 8..4 * 9], &[30, 210, 70, 255]);
    assert_eq!(&bytes[4 * 24..4 * 25], &[20, 50, 220, 255]);

    let replacement = create_test_target_texture(&renderer, 32, 16, "replacement history test");
    renderer
        .render_to_persistent_wgpu_texture(
            &second,
            &replacement,
            crate::ExternalTextureHistoryId::new(2),
        )
        .expect("new external identity must rebuild its contents");
    assert_eq!(
        renderer.incremental_render_stats().full_redraw_reason,
        Some(crate::FullRedrawReason::FirstFrame)
    );
    let bytes = read_texture_rgba8(renderer.device(), renderer.queue(), &replacement, 32, 16);
    assert_eq!(&bytes[4 * 8..4 * 9], &[30, 210, 70, 255]);
    assert_eq!(&bytes[4 * 24..4 * 25], &[20, 50, 220, 255]);
}

#[test]
fn retained_filter_surface_is_reused_when_damage_is_elsewhere() {
    if !run_wgpu_tests() {
        return;
    }

    let mut filtered = Canvas::new(40, 40, 1.0);
    filtered.push_filter_layer(
        Filter::Blur {
            std_dev_x: 2.0,
            std_dev_y: 2.0,
            sampling: BlurSampling::default(),
        },
        Region::rect(Rect::new(4.0, 4.0, 28.0, 28.0), crate::Radius::ZERO),
    );
    filtered.push_rect(
        Rect::new(8.0, 8.0, 24.0, 24.0),
        crate::Radius::ZERO,
        Color::from_rgb8(220, 40, 80),
    );
    filtered.pop_layer();
    let filtered = std::sync::Arc::new(filtered);

    fn marker(color: Color) -> std::sync::Arc<Canvas> {
        let mut scene = Canvas::new(16, 16, 1.0);
        scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        std::sync::Arc::new(scene)
    }

    let root = RetainedNodeId::for_owner(10);
    let filter_id = RetainedNodeId::for_owner(11);
    let marker_id = RetainedNodeId::for_owner(12);
    let mut first = Canvas::new_retained(128, 64, 1.0, root);
    first.append_retained_scene(filter_id, 0, filtered.clone(), (0.0, 0.0));
    first.append_retained_scene(
        marker_id,
        0,
        marker(Color::from_rgb8(20, 40, 200)),
        (96.0, 0.0),
    );
    let mut second = Canvas::new_retained(128, 64, 1.0, root);
    second.append_retained_scene(filter_id, 0, filtered, (0.0, 0.0));
    second.append_retained_scene(
        marker_id,
        1,
        marker(Color::from_rgb8(20, 200, 40)),
        (96.0, 0.0),
    );

    let mut renderer = new_test_renderer(128, 64, Color::TRANSPARENT);
    renderer.render(&first);
    renderer.render(&second);
    assert!(!renderer.incremental_render_stats().full_redraw);
    assert_eq!(
        renderer
            .incremental_render_stats()
            .reused_offscreen_surfaces,
        1
    );
    assert_eq!(
        renderer
            .incremental_render_stats()
            .rerendered_offscreen_surfaces,
        0
    );

    let incremental = renderer.image();

    let mut evicted = new_test_renderer(128, 64, Color::TRANSPARENT);
    let mut evicted_config = evicted.incremental_render_config();
    evicted_config.retained_texture_budget_bytes = 0;
    evicted.set_incremental_render_config(evicted_config);
    evicted.render(&first);
    evicted.render(&second);
    assert_eq!(
        evicted.incremental_render_stats().reused_offscreen_surfaces,
        0
    );
    assert_eq!(
        evicted
            .incremental_render_stats()
            .rerendered_offscreen_surfaces,
        1
    );

    let mut full = new_test_renderer(128, 64, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&second);
    assert_eq!(incremental.pixels, full.image().pixels);
    assert_eq!(evicted.image().pixels, full.image().pixels);
}

#[test]
fn retained_filter_updates_local_dirty_tiles_and_matches_full_render() {
    if !run_wgpu_tests() {
        return;
    }

    fn frame(revision: u64, color: Color) -> Canvas {
        let root = RetainedNodeId::for_owner(20);
        let filter_node = RetainedNodeId::for_owner(21);
        let content_node = RetainedNodeId::for_owner(22);
        let mut content = Canvas::new(20, 20, 1.0);
        content.push_rect(Rect::new(2.0, 2.0, 18.0, 18.0), crate::Radius::ZERO, color);

        let mut canvas = Canvas::new_retained(160, 96, 1.0, root);
        canvas.push_retained_filter_layer(
            RetainedLayerKey::new(filter_node, crate::SceneRevision::INITIAL),
            Filter::Blur {
                std_dev_x: 2.0,
                std_dev_y: 2.0,
                sampling: BlurSampling::default(),
            },
            Region::rect(Rect::new(8.0, 8.0, 136.0, 72.0), crate::Radius::ZERO),
        );
        canvas.append_retained_scene(
            content_node,
            revision,
            std::sync::Arc::new(content),
            (40.0, 24.0),
        );
        canvas.pop_layer();
        canvas
    }

    let first = frame(0, Color::from_rgb8(220, 40, 30));
    let second = frame(1, Color::from_rgb8(20, 210, 50));
    let mut incremental = new_test_renderer(160, 96, Color::TRANSPARENT);
    incremental.render(&first);
    incremental.render(&second);
    let stats = incremental.incremental_render_stats().clone();
    assert!(!stats.full_redraw);
    assert_eq!(stats.rerendered_offscreen_surfaces, 1);
    assert!(
        stats.rerendered_offscreen_tiles < 45,
        "local filter should not redraw its full surface: {stats:?}"
    );

    let image = incremental.image();
    let mut full = new_test_renderer(160, 96, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&second);
    assert_eq!(image.pixels, full.image().pixels);
}

#[test]
fn retained_backdrop_blur_updates_local_tiles_and_matches_full_render() {
    if !run_wgpu_tests() {
        return;
    }

    fn frame(revision: u64, color: Color) -> Canvas {
        let mut canvas = Canvas::new_retained(128, 96, 1.0, RetainedNodeId::for_owner(30));
        let mut background = Canvas::new(128, 96, 1.0);
        background.push_rect(
            Rect::new(0.0, 0.0, 128.0, 96.0),
            crate::Radius::ZERO,
            Color::from_rgb8(32, 38, 48),
        );
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(31),
            0,
            std::sync::Arc::new(background),
            (0.0, 0.0),
        );
        let mut marker = Canvas::new(16, 16, 1.0);
        marker.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(32),
            revision,
            std::sync::Arc::new(marker),
            (48.0, 32.0),
        );
        canvas.push_retained_backdrop_layer(
            RetainedLayerKey::new(RetainedNodeId::for_owner(33), crate::SceneRevision::INITIAL),
            Filter::Blur {
                std_dev_x: 2.0,
                std_dev_y: 2.0,
                sampling: BlurSampling::FULL_RES,
            },
            Region::rect(Rect::new(16.0, 16.0, 112.0, 80.0), crate::Radius::all(8.0)),
        );
        canvas.pop_layer();
        canvas
    }

    let first = frame(0, Color::from_rgb8(230, 50, 40));
    let second = frame(1, Color::from_rgb8(30, 210, 90));
    let mut incremental = new_test_renderer(128, 96, Color::TRANSPARENT);
    incremental.render(&first);
    incremental.render(&second);
    let stats = incremental.incremental_render_stats().clone();
    assert_eq!(stats.rerendered_offscreen_surfaces, 1);
    assert!(stats.rerendered_offscreen_tiles < 35, "{stats:?}");

    let mut full = new_test_renderer(128, 96, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&second);
    let incremental_image = incremental.image();
    let full_image = full.image();
    let differences = incremental_image
        .pixels
        .iter()
        .zip(&full_image.pixels)
        .enumerate()
        .filter(|(_, (actual, expected))| actual != expected)
        .map(|(index, _)| ((index as u32) % 128, (index as u32) / 128))
        .collect::<Vec<_>>();
    assert!(
        differences.is_empty(),
        "backdrop differs at {} pixels, first {:?}",
        differences.len(),
        differences.first()
    );
}

#[test]
fn retained_clipped_liquid_glass_rerenders_after_backdrop_damage() {
    if !run_wgpu_tests() {
        return;
    }

    fn frame(revision: u64, color: Color) -> Canvas {
        let mut canvas = Canvas::new_retained(256, 192, 1.0, RetainedNodeId::for_owner(34));
        let mut background = Canvas::new(256, 192, 1.0);
        background.push_rect(
            Rect::new(0.0, 0.0, 256.0, 192.0),
            crate::Radius::ZERO,
            Color::from_rgb8(24, 32, 48),
        );
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(35),
            0,
            std::sync::Arc::new(background),
            (0.0, 0.0),
        );

        let mut marker = Canvas::new(16, 16, 1.0);
        marker.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(36),
            revision,
            std::sync::Arc::new(marker),
            (48.0, 32.0),
        );

        canvas.push_retained_clip_sdf_rect_layer(
            RetainedLayerKey::new(RetainedNodeId::for_owner(37), crate::SceneRevision::INITIAL),
            Rect::new(16.0, 16.0, 112.0, 80.0),
            crate::Radius::all(12.0),
        );
        canvas.push_backdrop_layer(
            Filter::RectLiquidGlass(RectLiquidGlass {
                blur_radius: 8,
                blur_sampling: BlurSampling::downsampled(2),
                ..RectLiquidGlass::default()
            }),
            Region::rect(Rect::new(24.0, 20.0, 104.0, 76.0), crate::Radius::all(10.0)),
        );
        canvas.pop_layer();
        canvas.pop_layer();
        canvas
    }

    let first = frame(0, Color::from_rgb8(220, 40, 60));
    let second = frame(1, Color::from_rgb8(30, 210, 100));
    let mut incremental = new_test_renderer(256, 192, Color::TRANSPARENT);
    incremental.render(&first);
    incremental.render(&second);
    let stats = incremental.incremental_render_stats();
    assert!(
        !stats.full_redraw,
        "second frame must exercise retained rerendering"
    );
    assert_eq!(stats.rerendered_offscreen_surfaces, 1);

    let mut full = new_test_renderer(256, 192, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&second);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn retained_liquid_glass_ignores_later_foreground_history_when_slider_moves() {
    if !run_wgpu_tests() {
        return;
    }

    fn frame(slider_x: f64) -> Canvas {
        let root = RetainedNodeId::for_owner(90);
        let background_id = RetainedNodeId::for_owner(91);
        let panel_id = RetainedNodeId::for_owner(92);
        let slider_id = RetainedNodeId::for_owner(93);
        let mut canvas = Canvas::new_retained(320, 192, 1.0, root);

        let mut background = Canvas::new(320, 192, 1.0);
        for x in (0..320).step_by(8) {
            let color = if x % 16 == 0 {
                Color::from_rgb8(24, 72, 120)
            } else {
                Color::from_rgb8(120, 56, 32)
            };
            background.push_rect(
                Rect::new(f64::from(x), 0.0, f64::from(x + 8), 192.0),
                crate::Radius::ZERO,
                color,
            );
        }
        canvas.append_retained_scene(
            background_id,
            0,
            std::sync::Arc::new(background),
            (0.0, 0.0),
        );

        let panel = Rect::new(24.0, 16.0, 296.0, 176.0);
        canvas.push_retained_backdrop_layer(
            RetainedLayerKey::new(panel_id, crate::SceneRevision::INITIAL),
            Filter::RectLiquidGlass(RectLiquidGlass {
                blur_radius: 5,
                blur_sampling: BlurSampling::downsampled(4),
                tint: Color::from_rgba8(255, 255, 255, 26),
                refraction_thickness: 28.0,
                refraction_factor: 2.5,
                refraction_dispersion: 10.0,
                ..RectLiquidGlass::default()
            }),
            Region::rect(panel, crate::Radius::all(28.0)),
        );
        canvas.pop_layer();

        let mut slider = Canvas::new(18, 18, 1.0);
        slider.push_rect(
            Rect::new(0.0, 0.0, 18.0, 18.0),
            crate::Radius::all(9.0),
            Color::WHITE,
        );
        canvas.append_retained_scene(slider_id, 0, std::sync::Arc::new(slider), (slider_x, 72.0));
        canvas
    }

    let mut incremental = new_test_renderer(320, 192, Color::TRANSPARENT);
    incremental.render(&frame(120.0));
    let mut full = new_test_renderer(320, 192, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);

    for slider_x in [132.0, 144.0, 156.0, 170.0] {
        let current = frame(slider_x);
        incremental.render(&current);
        assert!(
            !incremental.incremental_render_stats().full_redraw,
            "slider movement must exercise dirty-tile rendering"
        );
        let stats = incremental.incremental_render_stats();
        assert!(stats.filter_dispatches > 0);
        assert_eq!(
            stats.compact_filter_dispatches, stats.filter_dispatches,
            "every retained liquid-glass stage, including downsampled blur, must use one compact worklist dispatch"
        );

        full.render(&current);
        let incremental_image = incremental.image();
        let full_image = full.image();
        let difference = incremental_image
            .pixels
            .iter()
            .zip(&full_image.pixels)
            .position(|(actual, expected)| actual != expected);
        assert_eq!(
            difference, None,
            "retained liquid glass diverged after moving slider to {slider_x}"
        );
    }

    // Evicting the backdrop source history must fall back to a full root
    // redraw. Rebuilding it from a partial final-frame texture would recreate
    // the same foreground feedback this test guards against.
    let mut uncached = new_test_renderer(320, 192, Color::TRANSPARENT);
    let mut uncached_config = uncached.incremental_render_config();
    uncached_config.retained_texture_budget_bytes = 0;
    uncached.set_incremental_render_config(uncached_config);
    uncached.render(&frame(120.0));
    let final_frame = frame(170.0);
    uncached.render(&final_frame);
    assert!(uncached.incremental_render_stats().full_redraw);
    full.render(&final_frame);
    assert_eq!(uncached.image().pixels, full.image().pixels);
}

#[test]
fn retained_nested_liquid_glass_stays_stable_when_clipped_slider_moves() {
    if !run_wgpu_tests() {
        return;
    }

    fn background() -> std::sync::Arc<Canvas> {
        let mut background = Canvas::new(320, 192, 1.0);
        for y in (0..192).step_by(8) {
            for x in (0..320).step_by(8) {
                let color = if (x / 8 + y / 8) % 2 == 0 {
                    Color::from_rgb8(236, 242, 250)
                } else {
                    Color::from_rgb8(86, 132, 178)
                };
                background.push_rect(
                    Rect::new(
                        f64::from(x),
                        f64::from(y),
                        f64::from(x + 8),
                        f64::from(y + 8),
                    ),
                    crate::Radius::ZERO,
                    color,
                );
            }
        }
        std::sync::Arc::new(background)
    }

    fn frame(slider_x: f64, background: &std::sync::Arc<Canvas>) -> Canvas {
        let panel = Rect::new(24.0, 16.0, 296.0, 176.0);
        let mut canvas = Canvas::new_retained(320, 192, 1.0, RetainedNodeId::for_owner(100));
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(101),
            0,
            background.clone(),
            (0.0, 0.0),
        );

        canvas.push_retained_clip_sdf_rect_layer(
            RetainedLayerKey::new(
                RetainedNodeId::for_owner(102),
                crate::SceneRevision::INITIAL,
            ),
            panel,
            crate::Radius::all(20.0),
        );
        let mut panel_scene = Canvas::new(272, 160, 1.0);
        let local_panel = Rect::new(0.0, 0.0, 272.0, 160.0);
        panel_scene.push_backdrop_layer(
            Filter::RectLiquidGlass(RectLiquidGlass {
                blur_radius: 5,
                blur_sampling: BlurSampling::downsampled(4),
                tint: Color::from_rgba8(255, 255, 255, 26),
                refraction_thickness: 28.0,
                refraction_factor: 2.5,
                refraction_dispersion: 10.0,
                ..RectLiquidGlass::default()
            }),
            Region::rect(local_panel, crate::Radius::all(20.0)),
        );
        panel_scene.push_rect(
            local_panel,
            crate::Radius::all(20.0),
            Color::from_rgba8(255, 255, 255, 26),
        );
        panel_scene.pop_layer();
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(103),
            0,
            std::sync::Arc::new(panel_scene),
            (24.0, 16.0),
        );

        let mut track = Canvas::new(220, 6, 1.0);
        track.push_rect(
            Rect::new(0.0, 0.0, 220.0, 6.0),
            crate::Radius::all(3.0),
            Color::from_rgba8(40, 50, 64, 100),
        );
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(104),
            0,
            std::sync::Arc::new(track),
            (50.0, 93.0),
        );

        let mut thumb = Canvas::new(18, 18, 1.0);
        thumb.push_rect(
            Rect::new(0.0, 0.0, 18.0, 18.0),
            crate::Radius::all(9.0),
            Color::from_rgb8(24, 30, 40),
        );
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(105),
            0,
            std::sync::Arc::new(thumb),
            (slider_x, 87.0),
        );
        canvas.pop_layer();
        canvas
    }

    let background = background();
    let mut incremental = new_test_renderer(320, 192, Color::TRANSPARENT);
    incremental.render(&frame(64.0, &background));
    for slider_x in [80.0, 96.0, 112.0, 128.0, 144.0, 160.0] {
        incremental.render(&frame(slider_x, &background));
        let stats = incremental.incremental_render_stats();
        assert!(!stats.full_redraw, "slider movement must stay incremental");
        assert!(stats.reused_offscreen_surfaces > 0);
        assert_eq!(stats.compact_filter_dispatches, stats.filter_dispatches);
    }

    let final_frame = frame(160.0, &background);
    let mut full = new_test_renderer(320, 192, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&final_frame);

    let incremental_image = incremental.image();
    let full_image = full.image();
    let difference = incremental_image
        .pixels
        .iter()
        .zip(&full_image.pixels)
        .position(|(actual, expected)| actual != expected);
    assert_eq!(
        difference, None,
        "a later clipped slider must not feed tile-shaped history into a nested liquid-glass backdrop"
    );
}

#[test]
fn retained_backdrop_revision_rebuilds_same_bounds_region_mask() {
    if !run_wgpu_tests() {
        return;
    }

    fn frame(revision: u64, radius: f32) -> Canvas {
        let mut canvas = Canvas::new_retained(256, 192, 1.0, RetainedNodeId::for_owner(110));
        let mut background = Canvas::new(256, 192, 1.0);
        background.push_rect(
            Rect::new(0.0, 0.0, 128.0, 192.0),
            crate::Radius::ZERO,
            Color::from_rgb8(30, 90, 180),
        );
        background.push_rect(
            Rect::new(128.0, 0.0, 256.0, 192.0),
            crate::Radius::ZERO,
            Color::from_rgb8(220, 100, 30),
        );
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(111),
            0,
            std::sync::Arc::new(background),
            (0.0, 0.0),
        );

        canvas.push_retained_clip_sdf_rect_layer(
            RetainedLayerKey::new(RetainedNodeId::for_owner(112), 0.into()),
            Rect::new(16.0, 8.0, 144.0, 88.0),
            crate::Radius::all(24.0),
        );
        canvas.push_retained_backdrop_layer(
            RetainedLayerKey::new(RetainedNodeId::for_owner(113), revision.into()),
            Filter::Blur {
                std_dev_x: 3.0,
                std_dev_y: 3.0,
                sampling: BlurSampling::FULL_RES,
            },
            Region::rect(
                Rect::new(32.0, 16.0, 128.0, 80.0),
                crate::Radius::all(radius),
            ),
        );
        canvas.pop_layer();
        canvas.pop_layer();
        canvas
    }

    let first = frame(0, 0.0);
    let second = frame(1, 22.0);
    let mut incremental = new_test_renderer(256, 192, Color::TRANSPARENT);
    incremental.render(&first);
    incremental.render(&second);
    assert!(
        !incremental.incremental_render_stats().full_redraw,
        "{:?}",
        incremental.incremental_render_stats()
    );

    let mut full = new_test_renderer(256, 192, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&second);
    let incremental_image = incremental.image();
    let full_image = full.image();
    assert_eq!(
        incremental_image
            .pixels
            .iter()
            .zip(&full_image.pixels)
            .position(|(actual, expected)| actual != expected),
        None
    );
}

#[test]
fn retained_mask_updates_local_tiles_and_matches_full_render() {
    if !run_wgpu_tests() {
        return;
    }

    fn frame(revision: u64, color: Color) -> Canvas {
        let mut canvas = Canvas::new_retained(128, 96, 1.0, RetainedNodeId::for_owner(40));
        let mut mask_scene = Canvas::new(128, 96, 1.0);
        mask_scene.push_rect(
            Rect::new(16.0, 16.0, 112.0, 80.0),
            crate::Radius::all(12.0),
            Color::WHITE,
        );
        canvas.push_retained_mask_layer(
            RetainedLayerKey::new(RetainedNodeId::for_owner(41), crate::SceneRevision::INITIAL),
            mask_scene,
            Mask {
                region: Region::rect(Rect::new(16.0, 16.0, 112.0, 80.0), crate::Radius::all(12.0)),
                kind: MaskKind::Alpha,
            },
        );
        let mut content = Canvas::new(16, 16, 1.0);
        content.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO, color);
        canvas.append_retained_scene(
            RetainedNodeId::for_owner(42),
            revision,
            std::sync::Arc::new(content),
            (48.0, 32.0),
        );
        canvas.pop_layer();
        canvas
    }

    let first = frame(0, Color::from_rgb8(220, 50, 90));
    let second = frame(1, Color::from_rgb8(40, 200, 150));
    let mut incremental = new_test_renderer(128, 96, Color::TRANSPARENT);
    incremental.render(&first);
    incremental.render(&second);
    let stats = incremental.incremental_render_stats().clone();
    assert_eq!(stats.rerendered_offscreen_surfaces, 1);
    assert!(stats.rerendered_offscreen_tiles < 24, "{stats:?}");
    assert_eq!(stats.compact_filter_dispatches, stats.filter_dispatches);

    let mut full = new_test_renderer(128, 96, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    full.render(&second);
    assert_eq!(incremental.image().pixels, full.image().pixels);
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
fn retained_profile_breaks_out_collection_materialization_and_damage() {
    if !run_wgpu_tests() {
        return;
    }

    let mut child = Canvas::new(16, 16, 1.0);
    child.push_rect(
        Rect::new(2.0, 2.0, 14.0, 14.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 120, 220),
    );
    let mut canvas = Canvas::new_retained(16, 16, 1.0, RetainedNodeId::for_owner(300));
    canvas.append_retained_scene(
        RetainedNodeId::for_owner(301),
        0,
        std::sync::Arc::new(child),
        (0.0, 0.0),
    );
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);

    renderer.start_profile();
    renderer.render(&canvas);
    let profile = renderer.end_profile().clone();

    assert_profile_has(&profile, "retained.collect");
    assert_profile_has(&profile, "retained.materialize");
    assert_profile_has(&profile, "retained.damage");
    let stats = profile
        .incremental_stats()
        .expect("incremental diagnostics");
    assert!(!stats.materialized_scene_reused);
    assert_eq!(stats.root_draw_batches, 1);
    assert_eq!(stats.draw_batches, 1);
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

    let Some((device, queue)) = portable_wgpu_device() else {
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
    let mut renderer = Renderer::new(&device, &queue, 16, 16, Color::TRANSPARENT);
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
    let Some((device, queue)) = portable_wgpu_device() else {
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
    let mut renderer = Renderer::new(&device, &queue, 16, 16, Color::TRANSPARENT);
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
    if std::env::var("TILEINK_WGPU_MODE").as_deref() != Ok("portable") {
        return Renderer::new_default_device(width, height, clear);
    }
    let Some((device, queue)) = portable_wgpu_device() else {
        return Renderer::new_default_device(width, height, clear);
    };
    Renderer::new(&device, &queue, width, height, clear)
}

fn portable_wgpu_device() -> Option<(::wgpu::Device, ::wgpu::Queue)> {
    let instance = ::wgpu::Instance::new(::wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&::wgpu::RequestAdapterOptions {
        power_preference: ::wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&::wgpu::DeviceDescriptor {
        label: Some("tileink portable wgpu test device"),
        required_features: ::wgpu::Features::empty(),
        required_limits: adapter.limits(),
        memory_hints: ::wgpu::MemoryHints::Performance,
        trace: ::wgpu::Trace::Off,
        experimental_features: ::wgpu::ExperimentalFeatures::disabled(),
    }))
    .ok()
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
    let (draws, layer_stack) = renderer
        .plan
        .as_ref()
        .expect("prepared execution plan")
        .ops
        .iter()
        .find_map(|op| match op {
            ExecOp::DrawBatch { draws, layer_stack } => Some((draws.clone(), layer_stack.clone())),
            _ => None,
        })
        .expect("execution plan contains a draw batch");
    // Path clips in the fused stack consume scan/cumsum backdrops before
    // coarse can decide whether the clip is a no-op or needs wrapper particles.
    renderer.scan_for_test();
    renderer.cumsum_for_test();
    renderer.coarse_batch(
        canvas,
        draws.start as u32,
        draws.end as u32,
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
