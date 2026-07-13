use super::*;

#[test]
fn fragmented_buffer_upload_scatter_matches_the_source_ranges() {
    if !run_wgpu_tests() {
        return;
    }

    let renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    let pipeline = std::rc::Rc::new(WgpuRangeScatterPipeline::new(
        &renderer.device,
        None,
        &PipelineCompilationTracker::default(),
    ));
    let mut scatter = WgpuRangeScatter::new(pipeline);
    let mut buffer = WgpuBuffer::new(&renderer.device, "range scatter test destination");
    let mut data = vec![0u32; 1_200];
    buffer.upload_ranges(
        &renderer.device,
        &renderer.queue,
        &mut scatter,
        "range scatter test destination",
        &data,
        &[],
    );

    let ranges = [0..1, 300..301, 600..601, 900..901];
    for (value, range) in ranges.iter().enumerate() {
        data[range.start] = value as u32 + 1;
    }
    buffer.upload_ranges(
        &renderer.device,
        &renderer.queue,
        &mut scatter,
        "range scatter test destination",
        &data,
        &ranges,
    );
    scatter.submit(&renderer.queue);

    assert_eq!(
        buffer.read::<u32>(&renderer.device, &renderer.queue, data.len()),
        data
    );
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

pub(super) fn render_resource_atlas_test(canvas: Canvas, key: ImageKey) -> Image {
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
