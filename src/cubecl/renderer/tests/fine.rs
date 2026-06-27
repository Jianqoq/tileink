use super::*;

#[test]
fn fine_wgpu_renders_solid_color_particles_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let blue = Color::from_rgb8(0, 0, 255);
    let mut scene = Scene::new(32, 16);
    scene.push_rect(Rect::new(0.0, 0.0, 32.0, 16.0), red, FillRule::NonZero);
    scene.push_rect(Rect::new(16.0, 0.0, 32.0, 16.0), blue, FillRule::NonZero);

    let mut renderer = WgpuRenderer::new_default_device(32, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[0], premul_f32_to_u32(red.premultiply().components));
    assert_eq!(target[15], premul_f32_to_u32(red.premultiply().components));
    assert_eq!(target[16], premul_f32_to_u32(blue.premultiply().components));
    assert_eq!(target[31], premul_f32_to_u32(blue.premultiply().components));
}

#[test]
fn fine_wgpu_samples_linear_gradient_brush_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let gradient = Gradient::new_linear((0.0, 0.0), (31.0, 0.0))
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let mut scene = Scene::new(32, 16);
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        &gradient,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(32, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let left = unpack_rgba8(target[8 * 32 + 2]);
    let right = unpack_rgba8(target[8 * 32 + 29]);

    assert!(left[0] > left[2], "expected red side, got {left:?}");
    assert!(right[2] > right[0], "expected blue side, got {right:?}");
    assert_eq!(left[3], 255);
    assert_eq!(right[3], 255);
}

#[test]
fn fine_wgpu_samples_radial_sweep_and_four_corner_brushes_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let radial = Gradient::new_radial((16.0, 16.0), 12.0)
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let sweep = Gradient::new_sweep((48.0, 16.0), 0.0, std::f32::consts::TAU)
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 255, 0)]);
    let four_corner = Brush::four_corner(
        Rect::new(64.0, 0.0, 96.0, 32.0),
        [
            Color::from_rgb8(255, 0, 0),
            Color::from_rgb8(0, 255, 0),
            Color::from_rgb8(0, 0, 255),
            Color::from_rgb8(255, 255, 0),
        ],
    );
    let mut scene = Scene::new(96, 32);
    scene.push_rect(Rect::new(0.0, 0.0, 32.0, 32.0), &radial, FillRule::NonZero);
    scene.push_rect(Rect::new(32.0, 0.0, 64.0, 32.0), &sweep, FillRule::NonZero);
    scene.push_rect(
        Rect::new(64.0, 0.0, 96.0, 32.0),
        four_corner,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(96, 32, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let radial_center = unpack_rgba8(target[16 * 96 + 16]);
    let radial_edge = unpack_rgba8(target[16 * 96 + 28]);
    let sweep_right = unpack_rgba8(target[16 * 96 + 60]);
    let four_corner_top_left = unpack_rgba8(target[2 * 96 + 66]);
    let four_corner_bottom_right = unpack_rgba8(target[29 * 96 + 93]);

    assert!(
        radial_center[0] > radial_center[2],
        "expected red radial center, got {radial_center:?}"
    );
    assert!(
        radial_edge[2] > radial_edge[0],
        "expected blue radial edge, got {radial_edge:?}"
    );
    assert!(
        sweep_right[0] > sweep_right[1],
        "expected red sweep sample, got {sweep_right:?}"
    );
    assert!(
        four_corner_top_left[0] > four_corner_top_left[1],
        "expected red top-left four-corner sample, got {four_corner_top_left:?}"
    );
    assert!(
        four_corner_bottom_right[2] > four_corner_bottom_right[1],
        "expected blue bottom-right four-corner sample, got {four_corner_bottom_right:?}"
    );
}

#[test]
fn fine_wgpu_rasterizes_fill_particles_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let mut scene = Scene::new(16, 16);
    scene.push_path(
        Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
        red,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(
        target[8 * 16 + 8],
        premul_f32_to_u32(red.premultiply().components)
    );
}

#[test]
fn fine_wgpu_applies_clip_particles_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let mut scene = Scene::new(16, 16);
    scene.push_clip_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
    );
    scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(
        target[8 * 16 + 4],
        premul_f32_to_u32(red.premultiply().components)
    );
    assert_eq!(target[8 * 16 + 12], 0);
}

#[test]
fn fine_wgpu_applies_opacity_layer_stack_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let mut scene = Scene::new(16, 16);
    scene.push_opacity_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        0.5,
    );
    scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[8 * 16 + 4], rgba8_pack([128, 0, 0, 128]));
    assert_eq!(target[8 * 16 + 12], 0);
}

#[test]
fn fine_wgpu_applies_blend_layer_stack_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let blue = Color::from_rgb8(0, 0, 255);
    let mut scene = Scene::new(16, 16);
    scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), blue, FillRule::NonZero);
    scene.push_blend_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[8 * 16 + 4], rgba8_pack([0, 0, 0, 255]));
    assert_eq!(
        target[8 * 16 + 12],
        premul_f32_to_u32(blue.premultiply().components)
    );
}

#[test]
fn fine_wgpu_does_not_leak_clip_after_layer_pop_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let blue = Color::from_rgb8(0, 0, 255);
    let mut scene = Scene::new(16, 16);
    scene.push_clip_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
    );
    scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);
    scene.pop_layer();
    scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), blue, FillRule::NonZero);

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(
        target[8 * 16 + 4],
        premul_f32_to_u32(blue.premultiply().components)
    );
    assert_eq!(
        target[8 * 16 + 12],
        premul_f32_to_u32(blue.premultiply().components)
    );
}

#[test]
fn fine_wgpu_intersects_nested_clip_layers_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let mut scene = Scene::new(16, 16);
    scene.push_clip_layer(
        Rect::new(0.0, 0.0, 12.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
    );
    scene.push_clip_layer(
        Rect::new(4.0, 0.0, 16.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
    );
    scene.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), red, FillRule::NonZero);
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let red_px = premul_f32_to_u32(red.premultiply().components);

    assert_eq!(target[8 * 16 + 2], 0);
    assert_eq!(target[8 * 16 + 8], red_px);
    assert_eq!(target[8 * 16 + 14], 0);
}

#[test]
fn fine_wgpu_uses_premultiplied_clear_color_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let clear = Color::from_rgba8(255, 0, 0, 128);
    let scene = Scene::new(4, 4);
    let mut renderer = WgpuRenderer::new_default_device(4, 4, clear);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[0], premul_f32_to_u32(clear.premultiply().components));
}
