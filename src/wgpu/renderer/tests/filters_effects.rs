use super::*;

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
