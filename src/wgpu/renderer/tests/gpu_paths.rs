use super::*;

#[test]
fn transformed_sdf_rect_preserves_brush_and_device_space_coverage() {
    if !run_wgpu_tests() {
        return;
    }

    const WIDTH: u32 = 224;
    const HEIGHT: u32 = 224;
    let solid = Color::from_rgb8(32, 160, 224);
    let stops = [Color::from_rgb8(240, 48, 32), Color::from_rgb8(32, 96, 240)];

    let mut direct = Canvas::new(WIDTH, HEIGHT, 1.0);
    direct.push_rect(
        Rect::new(12.0, 8.0, 212.0, 24.0),
        crate::Radius::ZERO,
        solid,
    );
    let direct_gradient = Gradient::new_linear((12.0, 0.0), (212.0, 0.0)).with_stops(stops);
    direct.push_rect(
        Rect::new(12.0, 40.0, 212.0, 56.0),
        crate::Radius::ZERO,
        Brush::from_gradient(&direct_gradient),
    );
    direct.push_rect(
        Rect::new(100.0, 12.0, 116.0, 212.0),
        crate::Radius::ZERO,
        solid,
    );
    direct.push_rect(
        Rect::new(156.25, 96.25, 164.25, 104.25),
        crate::Radius::ZERO,
        solid,
    );

    let mut solid_unit = Canvas::new(1, 16, 1.0);
    solid_unit.push_rect(Rect::new(0.0, 0.0, 1.0, 16.0), crate::Radius::ZERO, solid);
    let local_gradient = Gradient::new_linear((0.0, 0.0), (1.0, 0.0)).with_stops(stops);
    let mut gradient_unit = Canvas::new(1, 16, 1.0);
    gradient_unit.push_rect(
        Rect::new(0.0, 0.0, 1.0, 16.0),
        crate::Radius::ZERO,
        Brush::from_gradient(&local_gradient),
    );
    let mut transformed = Canvas::new(WIDTH, HEIGHT, 1.0);
    transformed.append_transformed(
        &solid_unit,
        Affine::translate((12.0, 8.0)) * Affine::scale_non_uniform(200.0, 1.0),
    );
    transformed.append_transformed(
        &gradient_unit,
        Affine::translate((12.0, 40.0)) * Affine::scale_non_uniform(200.0, 1.0),
    );
    transformed.append_transformed(
        &solid_unit,
        // x' = 116 - y, y' = 12 + 200x: exact rotation verifies Jacobian orientation.
        Affine::new([0.0, 200.0, -1.0, 0.0, 116.0, 12.0]),
    );
    transformed
        .push_line(
            crate::SdfLine::new(
                peniko::kurbo::Point::new(160.25, 100.25),
                peniko::kurbo::Point::new(160.25, 100.25),
                8.0,
                crate::SdfLineCap::Square,
            ),
            solid,
        )
        .expect("zero-length square-cap line must be drawable");

    let mut direct_renderer = new_test_renderer(WIDTH, HEIGHT, Color::TRANSPARENT);
    direct_renderer.render(&direct);
    let mut transformed_renderer = new_test_renderer(WIDTH, HEIGHT, Color::TRANSPARENT);
    transformed_renderer.render(&transformed);

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let actual = transformed_renderer.image().rgba8_at(x, y);
            let expected = direct_renderer.image().rgba8_at(x, y);
            for channel in 0..4 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= 1,
                    "transformed pixel ({x}, {y}) differs: actual={actual:?}, expected={expected:?}"
                );
            }
        }
    }
}

#[test]
fn forced_coarse_binning_modes_render_the_same_incremental_frame() {
    if !run_wgpu_tests() {
        return;
    }

    let root = RetainedNodeId::for_owner(50_271);
    let background = RetainedNodeId::for_owner(50_272);
    let moving = RetainedNodeId::for_owner(50_273);
    let solid = |width, height, color| {
        let mut canvas = Canvas::new(width, height, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
            crate::Radius::ZERO,
            color,
        );
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(128, 128, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background,
            solid(128, 128, Color::from_rgb8(24, 48, 96)),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            moving,
            solid(48, 48, Color::from_rgb8(224, 96, 32)),
            Affine::translate((16.0, 24.0)),
        )
        .commit()
        .unwrap();

    let mut compact = new_test_renderer(128, 128, Color::TRANSPARENT);
    let mut compact_config = compact.incremental_render_config();
    compact_config.coarse_binning = crate::CoarseBinningMode::ForceCompact;
    compact.set_incremental_render_config(compact_config);
    let mut dense = new_test_renderer(128, 128, Color::TRANSPARENT);
    let mut dense_config = dense.incremental_render_config();
    dense_config.coarse_binning = crate::CoarseBinningMode::ForceDense;
    dense.set_incremental_render_config(dense_config);
    compact.render_retained(&scene);
    dense.render_retained(&scene);

    scene
        .transaction()
        .set_transform(moving, Affine::translate((48.0, 24.0)))
        .commit()
        .unwrap();
    compact.render_retained(&scene);
    dense.render_retained(&scene);

    assert_eq!(compact.image().pixels, dense.image().pixels);
    assert_eq!(compact.incremental_render_stats().dense_coarse_batches, 0);
    assert!(dense.incremental_render_stats().dense_coarse_batches > 0);
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
