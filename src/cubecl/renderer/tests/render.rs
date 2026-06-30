use super::*;

#[test]
fn render_wgpu_matches_cpu_for_text_layout_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut text_context = TextContext::new();
    let layout = text_context.layout(TextLayoutOptions::new("GPU text", 28.0));
    if layout.is_empty() {
        return;
    }

    let mut scene = Scene::new(160, 64);
    scene.push_rect(
        Rect::new(0.0, 0.0, 160.0, 64.0),
        crate::Radius::ZERO,
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.push_text_layout(&layout, peniko::kurbo::Point::new(8.0, 36.0), Color::BLACK);

    let mut cpu = CpuRenderer::new(160, 64, Color::WHITE);
    cpu.render_with_text(&scene, &mut text_context);

    let mut wgpu = WgpuRenderer::new_default_device(160, 64, Color::WHITE);
    wgpu.render_with_text(&scene, &mut text_context);

    assert_images_close(cpu.image(), &wgpu.image(), 1);
}

#[test]
fn render_wgpu_matches_cpu_for_emoji_text_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut text_context = TextContext::new();
    let layout = text_context.layout(TextLayoutOptions::new("Emoji 😀 👍🏽", 32.0));
    if layout.is_empty() {
        return;
    }

    let mut scene = Scene::new(192, 64);
    scene.push_rect(
        Rect::new(0.0, 0.0, 192.0, 64.0),
        crate::Radius::ZERO,
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.push_text_layout(&layout, peniko::kurbo::Point::new(8.0, 42.0), Color::BLACK);

    let mut cpu = CpuRenderer::new(192, 64, Color::WHITE);
    cpu.render_with_text(&scene, &mut text_context);

    let mut wgpu = WgpuRenderer::new_default_device(192, 64, Color::WHITE);
    wgpu.render_with_text(&scene, &mut text_context);

    assert_images_close(cpu.image(), &wgpu.image(), 1);
}

#[test]
fn render_wgpu_matches_cpu_for_path_text_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut text_context = TextContext::new();
    let layout = text_context.layout(TextLayoutOptions::new("Path text", 40.0));
    if layout.is_empty() {
        return;
    }

    let mut scene = Scene::new(220, 80);
    scene.push_rect(
        Rect::new(0.0, 0.0, 220.0, 80.0),
        crate::Radius::ZERO,
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.push_text_layout_as_path(
        &mut text_context,
        &layout,
        peniko::kurbo::Point::new(8.0, 56.0),
        Color::BLACK,
        Affine::IDENTITY,
        0.1,
    );

    let mut cpu = CpuRenderer::new(220, 80, Color::WHITE);
    cpu.render(&scene);

    let mut wgpu = WgpuRenderer::new_default_device(220, 80, Color::WHITE);
    wgpu.render(&scene);

    assert_images_close(cpu.image(), &wgpu.image(), 1);
}

#[test]
fn render_wgpu_matches_cpu_for_multi_tile_mixed_shapes_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let scene = mixed_shape_scene();
    let mut cpu = CpuRenderer::new(360, 260, Color::WHITE);
    cpu.render(&scene);

    let mut wgpu = WgpuRenderer::new_default_device(360, 260, Color::WHITE);
    wgpu.render(&scene);

    assert_images_close(cpu.image(), &wgpu.image(), 1);
}

#[test]
fn render_wgpu_matches_cpu_for_scaled_stroked_frame_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(300, 300);
    scene.push_stroke(
        Rect::new(1.0, 1.0, 199.0, 199.0),
        Stroke::new(1.0),
        Color::BLACK,
        Affine::scale(1.5),
        FillRule::NonZero,
        0.1,
    );

    let mut cpu = CpuRenderer::new(300, 300, Color::TRANSPARENT);
    cpu.render(&scene);
    let mut wgpu = WgpuRenderer::new_default_device(300, 300, Color::TRANSPARENT);
    wgpu.render(&scene);

    assert_images_close(cpu.image(), &wgpu.image(), 0);
}

#[test]
fn render_wgpu_matches_cpu_for_sdf_rect_shadow_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(96, 72);
    scene.push_rect_shadow(
        Rect::new(20.0, 16.0, 56.0, 44.0),
        Radius::all(8.0),
        RectShadowOptions::new(6.0, 5.0, 6.0, 0.45),
        Color::BLACK,
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(20.0, 16.0, 56.0, 44.0),
        crate::Radius::ZERO,
        Color::from_rgb8(230, 90, 80),
        FillRule::NonZero,
    );

    let mut cpu = CpuRenderer::new(96, 72, Color::WHITE);
    cpu.render(&scene);
    let mut wgpu = WgpuRenderer::new_default_device(96, 72, Color::WHITE);
    wgpu.render(&scene);

    assert_images_close(cpu.image(), &wgpu.image(), 1);
}

#[test]
fn render_wgpu_matches_cpu_for_sdf_shape_shadows_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(128, 96);
    scene.push_circle_shadow(
        Circle::new((28.0, 28.0), 10.0),
        RectShadowOptions::new(5.0, 4.0, 5.0, 0.4),
        Color::BLACK,
        FillRule::NonZero,
    );
    scene.push_circle(
        Circle::new((28.0, 28.0), 10.0),
        Color::from_rgb8(0, 128, 255),
        FillRule::NonZero,
    );
    let arc = SdfArc::new(
        Point::new(70.0, 34.0),
        14.0,
        0.0,
        std::f32::consts::FRAC_PI_2,
        4.0,
        SdfLineCap::Round,
    );
    scene.push_arc_shadow(
        arc,
        RectShadowOptions::new(4.0, 5.0, 4.0, 0.45),
        Color::BLACK,
        FillRule::NonZero,
    );
    scene.push_sdf_arc(arc, Color::from_rgb8(220, 64, 72), FillRule::NonZero);
    let line = SdfLine::new(
        Point::new(18.0, 70.5),
        Point::new(92.0, 70.5),
        2.0,
        SdfLineCap::Square,
    );
    scene.push_line_shadow(
        line,
        RectShadowOptions::new(3.0, 5.0, 4.0, 0.45),
        Color::BLACK,
        FillRule::NonZero,
    );
    scene.push_line(line, Color::from_rgb8(34, 197, 94), FillRule::NonZero);

    let mut cpu = CpuRenderer::new(128, 96, Color::WHITE);
    cpu.render(&scene);
    let mut wgpu = WgpuRenderer::new_default_device(128, 96, Color::WHITE);
    wgpu.render(&scene);

    assert_images_close(cpu.image(), &wgpu.image(), 1);
}

#[test]
fn render_wgpu_matches_cpu_for_sdf_clip_layer_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(96, 72);
    scene.push_clip_sdf_rect_layer(Rect::new(16.0, 12.0, 80.0, 60.0), Radius::all(14.0));
    scene.push_rect(
        Rect::new(0.0, 0.0, 96.0, 72.0),
        Radius::ZERO,
        Color::from_rgb8(34, 197, 94),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut cpu = CpuRenderer::new(96, 72, Color::TRANSPARENT);
    cpu.render(&scene);
    let mut wgpu = WgpuRenderer::new_default_device(96, 72, Color::TRANSPARENT);
    wgpu.render(&scene);

    assert_eq!(wgpu.image().rgba8_at(8, 8), [0, 0, 0, 0]);
    assert_eq!(wgpu.image().rgba8_at(48, 36), [34, 197, 94, 255]);
    assert_images_close(cpu.image(), &wgpu.image(), 1);
}

#[test]
fn render_wgpu_matches_cpu_for_sdf_clip_inside_opacity_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(96, 72);
    scene.push_opacity_layer(
        Rect::new(0.0, 0.0, 96.0, 72.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        0.5,
    );
    scene.push_clip_sdf_circle_layer(Circle::new((48.0, 36.0), 24.0));
    scene.push_rect(
        Rect::new(0.0, 0.0, 96.0, 72.0),
        Radius::ZERO,
        Color::from_rgb8(34, 197, 94),
        FillRule::NonZero,
    );
    scene.pop_layer();
    scene.pop_layer();

    let mut cpu = CpuRenderer::new(96, 72, Color::TRANSPARENT);
    cpu.render(&scene);
    let mut wgpu = WgpuRenderer::new_default_device(96, 72, Color::TRANSPARENT);
    wgpu.render(&scene);

    assert_eq!(wgpu.image().rgba8_at(8, 8), [0, 0, 0, 0]);
    let center = wgpu.image().rgba8_at(48, 36);
    assert_eq!(&center[..3], &[17, 99, 47]);
    assert!((127..=128).contains(&center[3]), "{center:?}");
    assert_images_close(cpu.image(), &wgpu.image(), 1);
}

#[test]
fn render_wgpu_debug_capture_reads_back_scan_and_final_image_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let scene = mixed_shape_scene();
    let options = crate::RenderOptions {
        debug: Some(crate::RenderDebugOptions::new("target/cubecl-debug-test").with_tile((5, 10))),
    };
    let mut renderer = WgpuRenderer::new_default_device(360, 260, Color::WHITE);

    let capture = renderer.render_with_options(&scene, &options);

    assert_eq!(capture.backend, "cubecl");
    assert!(
        capture
            .images
            .iter()
            .any(|image| image.name == "tile_final.png")
    );
    assert_eq!(
        capture.tiles.len(),
        scene.width_in_tiles() as usize * scene.height_in_tiles() as usize
    );
    let tile = capture.tile.as_ref().expect("specific tile dump");
    assert!(!tile.paths.is_empty());
    assert_eq!(tile.final_rgba[0], renderer.image().rgba8_at(80, 160));
}

#[test]
fn render_wgpu_fills_top_clipped_path_between_edge_tiles_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let scene = top_clipped_rect_scene();
    let mut renderer = WgpuRenderer::new_default_device(128, 48, Color::TRANSPARENT);

    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(64, 6), [0, 0, 255, 255]);
}

#[test]
fn render_wgpu_keeps_left_clipped_skew_edge_from_double_backdrop_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut path = BezPath::new();
    path.move_to((-90.0, 0.0));
    path.line_to((90.0, 0.0));
    path.line_to((304.515_66, 180.0));
    path.line_to((124.515_66, 180.0));
    path.close_path();

    let mut scene = Scene::new(180, 180);
    scene.push_path(
        path,
        Color::from_rgb8(0, 128, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    let mut renderer = WgpuRenderer::new_default_device(180, 180, Color::TRANSPARENT);

    renderer.render(&scene);

    assert_eq!(renderer.image().rgba8_at(161, 64), [0, 128, 0, 255]);
    assert_eq!(renderer.image().rgba8_at(175, 64), [0, 0, 0, 0]);
}
