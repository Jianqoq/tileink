use super::*;

#[cfg(feature = "profile")]
#[test]
fn render_wgpu_records_profile_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let scene = mixed_shape_scene();
    let mut renderer = WgpuRenderer::new_default_device(360, 260, Color::WHITE);
    renderer.start_profile();
    renderer.render(&scene);
    let profile = renderer.end_profile();
    let launches = profile
        .summary()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();

    assert!(profile.kernel_time() > std::time::Duration::ZERO);
    assert!(profile.memory_used_bytes() > 0);
    assert!(profile.memory_allocated_bytes() >= profile.memory_used_bytes());
    assert!(
        profile.memory_allocated_bytes_in(crate::RenderProfileMemorySpace::Gpu) > 0,
        "expected GPU memory snapshot"
    );
    assert!(
        profile.memory_allocated_bytes_in(crate::RenderProfileMemorySpace::Cpu) > 0,
        "expected CPU memory snapshot"
    );
    assert!(
        profile
            .memory_entries()
            .iter()
            .all(|entry| entry.allocated_bytes >= entry.used_bytes)
    );
    let memory_groups = profile
        .memory_entries()
        .iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    for group in ["scene", "scan", "coarse", "target"] {
        assert!(
            memory_groups.contains(&group),
            "missing profile memory group {group}"
        );
    }
    for launch in [
        "prepare_scene",
        "scan_clear",
        "scan_count",
        "scan_prefix_chunks",
        "scan_chunk_offsets",
        "scan_apply_chunk_offsets",
        "scan_emit",
        "cumsum_prefix_chunks",
        "fine_clear",
        "coarse_count",
        "coarse_ptcl_prefix_chunks",
        "coarse_ptcl_chunk_offsets",
        "coarse_ptcl_apply_chunk_offsets",
        "coarse_glyph_prefix_chunks",
        "coarse_glyph_chunk_offsets",
        "coarse_glyph_apply_chunk_offsets",
        "coarse_emit",
        "fine_render",
    ] {
        assert!(
            launches.contains(&launch),
            "missing profile launch {launch}"
        );
    }
    assert!(
        profile
            .entries()
            .iter()
            .any(|entry| entry.name == "fine_render" && entry.kernel_duration.is_some()),
        "expected fine_render to have kernel-only timing"
    );
    assert!(
        profile
            .entries()
            .iter()
            .any(|entry| entry.name == "prepare_scene" && entry.kernel_duration.is_none()),
        "prepare_scene should remain a CPU-side event"
    );

    let mut report = crate::RenderProfileReport::new();
    report.push(profile);
    assert_eq!(report.iterations(), 1);
    let report = report.to_string();
    assert!(report.contains("kernel us"));
    assert!(report.contains("kernel %"));
    assert!(!report.contains("wall us"));
    assert!(!report.contains("event %"));
    assert!(report.contains("memory"));
    assert!(report.contains("gpu"));
}

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

#[cfg(feature = "wgpu")]
#[test]
fn render_wgpu_blits_target_to_wgpu_texture_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let width = 8;
    let height = 4;
    let mut scene = Scene::new(width, height);
    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
        Color::from_rgba8(25, 150, 220, 192),
        FillRule::NonZero,
    );

    let setup = ::cubecl::wgpu::init_setup::<::cubecl::wgpu::AutoGraphicsApi>(
        &::cubecl::wgpu::WgpuDevice::DefaultDevice,
        ::cubecl::wgpu::RuntimeOptions::default(),
    );
    let device = setup.device.clone();
    let queue = setup.queue.clone();
    let cube_device = ::cubecl::wgpu::init_device(setup, ::cubecl::wgpu::RuntimeOptions::default());
    let mut renderer = WgpuRenderer::new(&cube_device, width, height, Color::TRANSPARENT);

    let dst = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tileink test target texture blit dst"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    renderer
        .render_to_wgpu_texture(&scene, &device, &queue, &dst)
        .expect("blit target to wgpu texture");

    assert_eq!(
        read_wgpu_texture(&device, &queue, &dst, width, height),
        renderer.image().rgba8_bytes()
    );
}

#[cfg(feature = "wgpu")]
fn read_wgpu_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let row_bytes = width as wgpu::BufferAddress * 4;
    let padded_row_bytes =
        row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as wgpu::BufferAddress);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tileink test texture readback"),
        size: padded_row_bytes * height as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("tileink test texture readback"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: (height > 1).then_some(padded_row_bytes as u32),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();

    let view = readback.slice(..).get_mapped_range();
    let mut tight = Vec::with_capacity((row_bytes * height as u64) as usize);
    for row in 0..height as usize {
        let start = row * padded_row_bytes as usize;
        tight.extend_from_slice(&view[start..start + row_bytes as usize]);
    }
    drop(view);
    readback.unmap();
    tight
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
