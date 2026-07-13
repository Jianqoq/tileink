use super::*;

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
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(16, 16, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            leaf(Color::WHITE),
            Affine::translate((0.0, 0.0)),
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
            std::rc::Rc::new(background),
            Affine::IDENTITY,
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
            std::rc::Rc::new(scene),
            Affine::IDENTITY,
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
            // GPU timestamp readback may or may not resolve before `end_profile` returns, so
            // count the synchronous CPU scopes that represent encoded copy passes.
            .filter(|entry| entry.name == "filter.copy" && entry.cpu_duration.is_some())
            .count(),
        immediate_profile
            .entries()
            .iter()
            .filter(|entry| entry.name == "filter.copy" && entry.cpu_duration.is_some())
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
