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
    renderer.render_flat(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[0], premul_f32_to_u32(red.premultiply().components));
    assert_eq!(target[15], premul_f32_to_u32(red.premultiply().components));
    assert_eq!(target[16], premul_f32_to_u32(blue.premultiply().components));
    assert_eq!(target[31], premul_f32_to_u32(blue.premultiply().components));
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
    renderer.render_flat(&scene);
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
    renderer.render_flat(&scene);
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
    renderer.render_flat(&scene);
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
    renderer.render_flat(&scene);
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
    renderer.render_flat(&scene);
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
    renderer.render_flat(&scene);
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
    renderer.render_flat(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[0], premul_f32_to_u32(clear.premultiply().components));
}
