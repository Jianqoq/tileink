use super::*;

#[test]
fn root_batch_submission_native_test_device_preserves_storage_capability() {
    if !run_wgpu_tests() {
        return;
    }
    let instance = ::wgpu::Instance::new(::wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&::wgpu::RequestAdapterOptions {
        power_preference: ::wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .unwrap();
    let Some((device, _)) = shared_wgpu_test_device(false) else {
        panic!("native test device must be available on this adapter");
    };
    // Without this capability the nominal native suite silently exercises portable fine.
    let formats = ::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    assert_eq!(
        device.features().contains(formats),
        adapter.features().contains(formats)
    );
}

fn layered_rects(size: u32, layers: u32, clip: bool) -> Canvas {
    let mut canvas = Canvas::new(size, size, 1.0);
    record_layered_rects(&mut canvas, size, layers, clip);
    canvas
}

fn record_layered_rects(canvas: &mut Canvas, size: u32, layers: u32, clip: bool) {
    for index in 0..layers {
        let inset = f64::from(index % 16);
        let rect = Rect::new(
            inset,
            inset,
            f64::from(size) - inset,
            f64::from(size) - inset,
        );
        if clip {
            canvas.push_clip_sdf_rect_layer(rect, crate::Radius::ZERO);
        }
        canvas.push_rect(
            rect,
            crate::Radius::ZERO,
            if index % 2 == 0 {
                Color::from_rgba8(220, 40, 80, 96)
            } else {
                Color::from_rgba8(20, 180, 140, 160)
            },
        );
        if clip {
            canvas.pop_layer();
        }
    }
}

#[test]
fn root_batch_submission_preserves_order_and_limits_extra_submissions() {
    if !run_wgpu_tests() {
        return;
    }
    let mut renderer = new_test_renderer(1024, 1024, Color::TRANSPARENT);
    let mut reference = new_test_renderer(1024, 1024, Color::TRANSPARENT);
    for (size, layers) in [
        (1024, 32),
        (256, 32),
        (1024, 4),
        (1024, 16),
        (1024, 64),
        (1024, 0),
    ] {
        renderer.render(&layered_rects(size, layers, true));
        reference.render(&layered_rects(size, layers, false));
        let stats = renderer.incremental_render_stats();
        assert_eq!(stats.root_draw_batches, layers);
        assert_eq!(
            reference.incremental_render_stats().root_draw_batches,
            u32::from(layers != 0)
        );
        // The oracle records the same integer-aligned rectangles in one batch. This checks
        // painter order and uniform-buffer reuse across submits, including transparent overlap.
        // Separate batches round intermediate RGBA8 values; the one-batch oracle rounds only
        // at the end. With these alpha values that accounts for at most two channel units.
        assert_images_near(
            &renderer.image(),
            &reference.image(),
            2,
            &format!("{size}/{layers}"),
        );
        let portable = renderer.fine.as_ref().unwrap().uses_portable_textures();
        assert_eq!(
            stats.queue_submissions,
            if !portable && size == 1024 && layers >= 16 {
                2
            } else {
                1
            },
            "only substantial native frames should submit early: {size}/{layers}, portable={portable}",
        );
    }
}

#[test]
fn root_batch_submission_keeps_partial_and_unchanged_history_incremental() {
    if !run_wgpu_tests() {
        return;
    }
    const SIZE: u32 = 1024;
    let mut renderer = new_test_renderer(SIZE, SIZE, Color::TRANSPARENT);
    let root = RetainedNodeId::for_owner(99_000);
    let node = |index: u32| RetainedNodeId::for_owner(99_001 + u64::from(index));
    let child = |index: u32, changed| {
        let x = f64::from(index % 8) * 112.0 + 16.0;
        let y = f64::from(index / 8) * 112.0 + 16.0;
        let rect = Rect::new(x, y, x + 32.0, y + 32.0);
        let mut canvas = Canvas::new(SIZE, SIZE, 1.0);
        canvas.push_clip_sdf_rect_layer(rect, crate::Radius::ZERO);
        canvas.push_rect(
            rect,
            crate::Radius::ZERO,
            if changed {
                Color::from_rgb8(0, 255, 0)
            } else {
                Color::from_rgb8(0, 0, 255)
            },
        );
        canvas.pop_layer();
        std::rc::Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(SIZE, SIZE, 1.0, root).unwrap();
    let mut transaction = scene.transaction();
    for index in 0..32 {
        transaction.insert_scene(
            RetainedParent::content(root),
            None,
            node(index),
            child(index, false),
            Affine::IDENTITY,
        );
    }
    transaction.commit().unwrap();
    let device = renderer.device.clone();
    let queue = renderer.queue.clone();
    let target = external_target(&device, (SIZE, SIZE), "early submission history");
    let history = crate::wgpu::renderer::ExternalTextureHistoryId::new(99);
    renderer
        .render_retained_to_persistent_wgpu_texture(&scene, &target, history)
        .unwrap();
    assert_eq!(renderer.incremental_render_stats().root_draw_batches, 32);
    scene
        .transaction()
        .replace_scene(node(0), child(0, true))
        .commit()
        .unwrap();
    renderer
        .render_retained_to_persistent_wgpu_texture(&scene, &target, history)
        .unwrap();
    let stats = renderer.incremental_render_stats();
    assert!(!stats.full_redraw);
    assert_eq!(stats.root_draw_batches, 1);
    assert_eq!(stats.queue_submissions, 1);
    let mut reference = new_test_renderer(SIZE, SIZE, Color::TRANSPARENT);
    reference.render(&scene.to_canvas());
    assert!(
        read_texture_rgba8(&device, &queue, &target, SIZE, SIZE) == reference.image().rgba8_bytes()
    );
    renderer
        .render_retained_to_persistent_wgpu_texture(&scene, &target, history)
        .unwrap();
    assert_eq!(renderer.incremental_render_stats().root_draw_batches, 0);
    assert_eq!(renderer.incremental_render_stats().queue_submissions, 0);
}

#[test]
fn root_batch_submission_preserves_backdrop_and_foreground_dependencies() {
    if !run_wgpu_tests() {
        return;
    }
    let Some((device, queue)) = shared_wgpu_test_device(true) else {
        return;
    };
    let mut renderer = new_test_renderer(1024, 1024, Color::TRANSPARENT);
    let mut reference = Renderer::new(device, queue, 1024, 1024, Color::TRANSPARENT);
    for inside_backdrop in [false, true] {
        let mut canvas = Canvas::new(1024, 1024, 1.0);
        if !inside_backdrop {
            record_layered_rects(&mut canvas, 1024, 32, true);
        }
        canvas.push_backdrop_layer(
            Filter::Blur {
                std_dev_x: 4.0,
                std_dev_y: 4.0,
                sampling: BlurSampling::downsampled(3),
            },
            Region::rect(Rect::new(4.0, 4.0, 128.0, 128.0), crate::Radius::all(6.0)),
        );
        if inside_backdrop {
            // Backdrop foreground stays on Main, so its batches must also count toward
            // eligibility and the initial budget; counting top-level ops alone misses it.
            record_layered_rects(&mut canvas, 1024, 32, true);
        }
        canvas.pop_layer();
        canvas.push_rect(
            Rect::new(32.0, 32.0, 96.0, 96.0),
            crate::Radius::ZERO,
            Color::from_rgba8(200, 200, 20, 128),
        );
        renderer.render(&canvas);
        reference.render(&canvas);
        assert!(renderer.incremental_render_stats().root_draw_batches >= 32);
        assert_eq!(reference.incremental_render_stats().queue_submissions, 1);
        assert_eq!(
            renderer.incremental_render_stats().queue_submissions,
            if renderer.fine.as_ref().unwrap().uses_portable_textures() {
                1
            } else {
                2
            },
            "inside_backdrop={inside_backdrop}",
        );
        // The portable path keeps one submission and provides an independent ordering oracle
        // for backdrop reads and foreground composites, with normal backend rounding tolerance.
        assert_images_near(
            &renderer.image(),
            &reference.image(),
            1,
            &format!("early-submit backdrop: inside={inside_backdrop}"),
        );
    }
}
