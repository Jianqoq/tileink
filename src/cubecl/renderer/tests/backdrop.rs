use super::*;

#[test]
fn backdrop_wgpu_applies_color_filter_to_existing_target_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.push_backdrop_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(4.0, 0.0, 12.0, 16.0), Radius::all(0.0)),
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[8 * 16 + 8], rgba8_pack([0, 255, 255, 255]));
    assert_eq!(target[8 * 16 + 2], rgba8_pack([255, 0, 0, 255]));
    assert_eq!(target[8 * 16 + 14], rgba8_pack([255, 0, 0, 255]));
}

#[test]
fn backdrop_wgpu_masks_blur_to_rect_sample_region_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(24, 16);
    scene.push_rect(
        Rect::new(8.0, 4.0, 16.0, 12.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.push_backdrop_layer(
        Filter::Blur {
            radius_x: 2.0,
            radius_y: 2.0,
        },
        Region::rect(Rect::new(8.0, 4.0, 16.0, 12.0), Radius::all(0.0)),
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(24, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[8 * 24 + 6], 0);
    assert!(unpack_rgba8(target[8 * 24 + 12])[3] > 0);
}

#[test]
fn backdrop_wgpu_masks_color_filter_to_path_sample_region_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut triangle = BezPath::new();
    triangle.move_to((4.0, 4.0));
    triangle.line_to((12.0, 4.0));
    triangle.line_to((4.0, 12.0));
    triangle.close_path();

    let mut scene = Scene::new(16, 16);
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.push_backdrop_layer(
        Filter::Invert(1.0),
        Region::path(triangle, Affine::IDENTITY, 0.0),
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    assert_eq!(
        renderer.filter_paths.range_starts.read(renderer.client()),
        vec![0]
    );
    assert_eq!(
        renderer.filter_paths.range_ends.read(renderer.client()),
        vec![3]
    );
    assert_eq!(
        renderer.filter_paths.p0x.read(renderer.client()),
        vec![1024, 3072, 1024]
    );
    assert_eq!(
        renderer.filter_paths.p1x.read(renderer.client()),
        vec![3072, 1024, 1024]
    );
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[6 * 16 + 6], rgba8_pack([0, 255, 255, 255]));
    assert_eq!(target[10 * 16 + 10], rgba8_pack([255, 0, 0, 255]));
}
