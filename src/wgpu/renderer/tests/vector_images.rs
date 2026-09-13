use super::*;

fn patterned_canvas() -> Canvas {
    let tree = usvg::Tree::from_str(r##"<svg xmlns="http://www.w3.org/2000/svg" width="35" height="19">
        <defs>
            <pattern id="r" width="3" height="2" patternUnits="userSpaceOnUse"><rect width="1" height="1" fill="red"/></pattern>
            <pattern id="g" width="3" height="2" patternUnits="userSpaceOnUse"><rect width="1" height="1" fill="lime"/></pattern>
        </defs>
        <rect width="18" height="19" fill="url(#r)"/>
        <rect x="18" width="17" height="19" fill="url(#g)"/>
        </svg>"##, &usvg::Options::default()).unwrap();
    let mut canvas = Canvas::new(35, 19, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 35.0, 19.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    canvas.push_svg(&tree).unwrap();
    canvas
}

fn assert_pattern_pixels(pixels: &[u8]) {
    assert_eq!(pixels.len(), 35 * 19 * 4);
    for y in 0..19 {
        for x in 0..35 {
            let expected = if x % 3 == 0 && y % 2 == 0 {
                if x < 18 {
                    [255, 0, 0, 255]
                } else {
                    [0, 255, 0, 255]
                }
            } else {
                [0, 0, 255, 255]
            };
            let start = (y * 35 + x) * 4;
            assert_eq!(&pixels[start..start + 4], &expected, "pixel {x},{y}");
        }
    }
}

#[test]
fn deferred_images_use_transparent_child_clear_and_one_frame_submission() {
    if !run_wgpu_tests() {
        return;
    }
    let canvas = patterned_canvas();
    let mut renderer = new_test_renderer(35, 19, Color::WHITE);
    renderer.render(&canvas);
    assert_eq!(renderer.incremental_render_stats().queue_submissions, 1);
    assert_pattern_pixels(bytemuck::cast_slice(&renderer.image().pixels));
    renderer.render(&canvas);
    assert_pattern_pixels(bytemuck::cast_slice(&renderer.image().pixels));
}

#[test]
fn deferred_images_are_populated_in_local_filter_resource_sets() {
    if !run_wgpu_tests() {
        return;
    }
    let wrap = |contents: &str| {
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="35" height="19">
        <defs><filter id="blur" filterUnits="userSpaceOnUse" x="-4" y="-4" width="43" height="27"><feGaussianBlur stdDeviation="1"/></filter></defs>
        <g filter="url(#blur)">{contents}</g></svg>"##
        )
    };
    let pattern = wrap(
        r##"<defs><pattern id="p" width="3" height="2" patternUnits="userSpaceOnUse"><rect width="1" height="1" fill="red"/></pattern></defs><rect width="35" height="19" fill="url(#p)"/>"##,
    );
    let geometry = (0..19)
        .step_by(2)
        .flat_map(|y| {
            (0..35)
                .step_by(3)
                .map(move |x| format!(r#"<rect x="{x}" y="{y}" width="1" height="1" fill="red"/>"#))
        })
        .collect::<String>();
    let explicit = wrap(&geometry);
    let mut images = Vec::new();
    for svg in [pattern, explicit] {
        let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap();
        let mut canvas = Canvas::new(35, 19, 1.0);
        canvas.push_svg(&tree).unwrap();
        let mut renderer = new_test_renderer(35, 19, Color::TRANSPARENT);
        renderer.render(&canvas);
        images.push(renderer.image());
    }
    assert_eq!(images[0].pixels, images[1].pixels);
}

#[test]
fn a_failed_parent_frame_retries_all_deferred_children() {
    if !run_wgpu_tests() {
        return;
    }
    let canvas = patterned_canvas();
    let mut renderer = new_test_renderer(35, 19, Color::WHITE);
    renderer.prepare_scene(&canvas);
    let sources: Vec<_> = renderer
        .scene_buffers
        .vector_image_upload()
        .unwrap()
        .requests
        .iter()
        .map(|request| request.source.upgrade().unwrap())
        .collect();
    assert_eq!(sources.len(), 2);
    let mut failed = WgpuCommandBatch::new(
        &renderer.device,
        &renderer.queue,
        "deferred children before parent failure",
    );
    assert!(renderer.encode_vector_images(&mut failed));
    assert!(
        failed
            .resource_writes
            .available(&renderer.scene_buffers.vector_image_upload().unwrap().ready)
    );
    // Exercise the same failure finish as a later parent operation: a submitted
    // prefix must not publish the whole frame's pending resource generations.
    failed.submit_current();
    assert_eq!(failed.finish_with_status(false), 1);
    assert!(
        !renderer
            .scene_buffers
            .vector_image_upload()
            .unwrap()
            .ready
            .is_submitted()
    );

    let mut poison = WgpuCommandBatch::new(
        &renderer.device,
        &renderer.queue,
        "invalidate aborted child pixels",
    );
    for source in &sources {
        let entry = renderer
            .vector_images
            .get_or_insert(source, || panic!("child must already be cached"));
        assert!(!entry.ready.is_submitted());
        assert!(
            entry
                .value
                .clear_render_target(&mut poison, RenderTargetId::Main, 0xffff_ffff)
        );
    }
    assert_eq!(poison.finish(), 1);

    // Retry without changing the Canvas. Reusing either poisoned child would
    // produce white pattern pixels, so the full image asserts actual GPU recovery.
    renderer.render(&canvas);
    assert_pattern_pixels(bytemuck::cast_slice(&renderer.image().pixels));
    for source in &sources {
        assert!(
            renderer
                .vector_images
                .get_or_insert(source, || panic!("child must remain cached"))
                .ready
                .is_submitted()
        );
    }
}
