use super::*;

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
