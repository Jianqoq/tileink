use super::*;

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
