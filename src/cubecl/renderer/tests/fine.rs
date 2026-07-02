use super::*;

#[test]
fn fine_wgpu_renders_solid_color_particles_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let blue = Color::from_rgb8(0, 0, 255);
    let mut scene = Scene::new(32, 16);
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        red,
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(16.0, 0.0, 32.0, 16.0),
        crate::Radius::ZERO,
        blue,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(32, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[0], premul_f32_to_u32(red.premultiply().components));
    assert_eq!(target[15], premul_f32_to_u32(red.premultiply().components));
    assert_eq!(target[16], premul_f32_to_u32(blue.premultiply().components));
    assert_eq!(target[31], premul_f32_to_u32(blue.premultiply().components));
}

#[test]
fn fine_wgpu_renders_circle_sdf_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let mut scene = Scene::new(32, 32);
    scene.push_circle(Circle::new((16.0, 16.0), 8.0), red, FillRule::NonZero);

    let mut renderer = WgpuRenderer::new_default_device(32, 32, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let red_px = premul_f32_to_u32(red.premultiply().components);

    assert_eq!(target[16 * 32 + 16], red_px);
    assert_eq!(target[16 * 32 + 3], 0);
}

#[test]
fn fine_wgpu_uses_sparse_sdf_ref_when_path_draw_precedes_sdf_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let blue = Color::from_rgb8(0, 96, 255);
    let mut scene = Scene::new(48, 32);
    scene.push_path(
        Rect::new(0.0, 0.0, 4.0, 4.0).to_path(0.0),
        red,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_circle(Circle::new((24.0, 16.0), 8.0), blue, FillRule::NonZero);

    let mut renderer = WgpuRenderer::new_default_device(48, 32, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(
        target[2 * 48 + 2],
        premul_f32_to_u32(red.premultiply().components)
    );
    assert_eq!(
        target[16 * 48 + 24],
        premul_f32_to_u32(blue.premultiply().components)
    );
    assert_eq!(target[16 * 48 + 8], 0);
}

#[test]
fn fine_wgpu_renders_rect_stroke_sdf_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let mut scene = Scene::new(64, 64);
    scene.push_rect_stroke(
        Rect::new(16.0, 16.0, 48.0, 48.0),
        Radius::ZERO,
        Stroke::new(6.0),
        red,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(64, 64, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let red_px = premul_f32_to_u32(red.premultiply().components);

    assert_eq!(target[32 * 64 + 16], red_px);
    assert_eq!(target[32 * 64 + 32], 0);
    assert_eq!(target[32 * 64 + 8], 0);
}

#[test]
fn fine_wgpu_renders_rect_stroke_sdf_with_per_side_widths_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let mut scene = Scene::new(64, 64);
    scene.push_rect_stroke_widths(
        Rect::new(20.0, 20.0, 44.0, 44.0),
        Radius::ZERO,
        StrokeWidths {
            top: 2.0,
            right: 8.0,
            bottom: 4.0,
            left: 12.0,
        },
        red,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(64, 64, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let red_px = premul_f32_to_u32(red.premultiply().components);

    assert_eq!(target[32 * 64 + 16], red_px);
    assert_eq!(target[32 * 64 + 46], red_px);
    assert_eq!(target[20 * 64 + 32], red_px);
    assert_eq!(target[44 * 64 + 32], red_px);
    assert_eq!(target[32 * 64 + 32], 0);
    assert_eq!(target[32 * 64 + 12], 0);
}

#[test]
fn fine_wgpu_renders_circle_stroke_sdf_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let blue = Color::from_rgb8(0, 128, 255);
    let mut scene = Scene::new(64, 64);
    scene.push_circle_stroke(
        Circle::new((32.0, 32.0), 14.0),
        Stroke::new(6.0),
        blue,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(64, 64, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let blue_px = premul_f32_to_u32(blue.premultiply().components);

    assert_eq!(target[32 * 64 + 18], blue_px);
    assert_eq!(target[32 * 64 + 32], 0);
    assert_eq!(target[32 * 64 + 10], 0);
}

#[test]
fn fine_wgpu_renders_candlestick_sdf_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(40, 36);
    scene.push_candlestick(
        CandleStick::new(16.5, 4.0, 28.0, 10.0, 22.0, 7),
        red,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(40, 36, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let red_px = premul_f32_to_u32(red.premultiply().components);

    assert_eq!(target[5 * 40 + 16], red_px);
    assert_eq!(target[5 * 40 + 14], 0);
    assert_eq!(target[12 * 40 + 13], red_px);
    assert_eq!(target[12 * 40 + 19], red_px);
    assert_eq!(target[12 * 40 + 21], 0);
    assert_eq!(target[29 * 40 + 16], 0);
}

#[test]
fn fine_wgpu_renders_line_sdf_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(40, 36);
    scene.push_line(
        SdfLine::new(
            Point::new(8.0, 16.5),
            Point::new(24.0, 16.5),
            1.0,
            SdfLineCap::Butt,
        ),
        red,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(40, 36, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let red_px = premul_f32_to_u32(red.premultiply().components);

    assert_eq!(target[16 * 40 + 8], red_px);
    assert_eq!(target[16 * 40 + 23], red_px);
    assert_eq!(target[16 * 40 + 7], 0);
    assert_eq!(target[16 * 40 + 24], 0);
    assert_eq!(target[15 * 40 + 16], 0);
    assert_eq!(target[17 * 40 + 16], 0);
}

#[test]
fn fine_wgpu_renders_dash_line_sdf_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(48, 36);
    scene.push_dash_line(
        SdfDashLine::new(
            Point::new(8.0, 16.5),
            Point::new(32.0, 16.5),
            1.0,
            SdfLineCap::Butt,
            4.0,
            3.0,
        ),
        red,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(48, 36, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let red_px = premul_f32_to_u32(red.premultiply().components);

    assert_eq!(target[16 * 48 + 8], red_px);
    assert_eq!(target[16 * 48 + 11], red_px);
    assert_eq!(target[16 * 48 + 12], 0);
    assert_eq!(target[16 * 48 + 14], 0);
    assert_eq!(target[16 * 48 + 15], red_px);
    assert_eq!(target[15 * 48 + 16], 0);
}

#[test]
fn fine_wgpu_renders_arc_sdf_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(220, 64, 72);
    let mut scene = Scene::new(64, 64);
    scene.push_sdf_arc(
        SdfArc::new(
            Point::new(32.0, 32.0),
            12.0,
            0.0,
            std::f32::consts::FRAC_PI_2,
            4.0,
            SdfLineCap::Round,
        ),
        red,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(64, 64, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let red_px = premul_f32_to_u32(red.premultiply().components);

    assert_eq!(target[40 * 64 + 40], red_px);
    assert_eq!(target[32 * 64 + 24], 0);
    assert_eq!(target[24 * 64 + 32], 0);
}

#[test]
fn fine_wgpu_preserves_rect_sdf_subpixel_coverage_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(32, 16);
    scene.push_rect(
        Rect::new(8.25, 0.0, 24.25, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(32, 16, Color::WHITE);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let edge = unpack_rgba8(target[8 * 32 + 8]);

    assert_eq!(unpack_rgba8(target[8 * 32 + 7]), [255, 255, 255, 255]);
    assert_eq!(unpack_rgba8(target[8 * 32 + 9]), [255, 0, 0, 255]);
    assert_eq!(edge[0], 255);
    assert_eq!(edge[1], edge[2]);
    assert!(
        edge[1] > 0 && edge[1] < 255,
        "expected partially covered SDF edge pixel, got {edge:?}"
    );
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
        crate::Radius::ZERO,
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
fn fine_wgpu_samples_bilinear_pattern_brush_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let brush = Brush::Pattern(PatternBrush {
        image: Arc::new(Image {
            width: 2,
            height: 1,
            pixels: vec![rgba8_pack([255, 0, 0, 255]), rgba8_pack([0, 0, 255, 255])],
        }),
        transform: [0.5, 0.0, 0.0, 0.5, 0.0, 0.0],
        extend: Extend::Pad,
        sampling: PatternSampling::Bilinear,
        opacity: 255,
    });
    let mut scene = Scene::new(4, 2);
    scene.push_rect(
        Rect::new(0.0, 0.0, 4.0, 2.0),
        crate::Radius::ZERO,
        brush,
        FillRule::NonZero,
    );

    let mut renderer = WgpuRenderer::new_default_device(4, 2, Color::TRANSPARENT);
    renderer.render(&scene);
    let edge = unpack_rgba8(renderer.target.read(renderer.client())[6]);

    assert_eq!(edge[3], 255);
    assert!(
        edge[0] > 0 && edge[2] > 0,
        "expected smoothed pattern edge, got {edge:?}"
    );
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
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 32.0),
        crate::Radius::ZERO,
        &radial,
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(32.0, 0.0, 64.0, 32.0),
        crate::Radius::ZERO,
        &sweep,
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(64.0, 0.0, 96.0, 32.0),
        crate::Radius::ZERO,
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
        FillRule::NonZero,
        0.0,
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        red,
        FillRule::NonZero,
    );

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
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        red,
        FillRule::NonZero,
    );
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
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        blue,
        FillRule::NonZero,
    );
    scene.push_blend_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        red,
        FillRule::NonZero,
    );
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
        FillRule::NonZero,
        0.0,
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        red,
        FillRule::NonZero,
    );
    scene.pop_layer();
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        blue,
        FillRule::NonZero,
    );

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
        FillRule::NonZero,
        0.0,
    );
    scene.push_clip_layer(
        Rect::new(4.0, 0.0, 16.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        red,
        FillRule::NonZero,
    );
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
fn fine_wgpu_spills_deep_clip_stack_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let red = Color::from_rgb8(255, 0, 0);
    let depth = crate::cubecl::pipelines::fine::FINE_LOCAL_CLIP_DEPTH + 2;
    let mut scene = Scene::new(16, 16);
    for inset in 0..depth {
        let inset = inset as f64;
        scene.push_clip_layer(
            Rect::new(inset, 0.0, 16.0 - inset, 16.0).to_path(0.0),
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );
    }
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        red,
        FillRule::NonZero,
    );
    for _ in 0..depth {
        scene.pop_layer();
    }

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let red_px = premul_f32_to_u32(red.premultiply().components);

    assert_eq!(target[8 * 16 + 4], 0);
    assert_eq!(target[8 * 16 + 8], red_px);
    assert_eq!(target[8 * 16 + 12], 0);
}

#[test]
fn fine_wgpu_spills_deep_opacity_blend_stack_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let depth = crate::cubecl::pipelines::fine::FINE_LOCAL_GROUP_DEPTH + 2;
    let mut scene = Scene::new(16, 16);
    scene.push_rect(
        full,
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
        FillRule::NonZero,
    );
    for i in 0..depth {
        if i % 2 == 0 {
            scene.push_opacity_layer(full.to_path(0.0), Affine::IDENTITY, 0.0, 0.75);
        } else {
            scene.push_blend_layer(
                full.to_path(0.0),
                Affine::IDENTITY,
                0.0,
                Mix::Multiply,
                Compose::SrcOver,
            );
        }
    }
    scene.push_rect(
        full,
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    for _ in 0..depth {
        scene.pop_layer();
    }

    let mut cpu = CpuRenderer::new(16, 16, Color::TRANSPARENT);
    cpu.render(&scene);
    let mut wgpu = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    wgpu.render(&scene);

    assert_images_close(&cpu.image(), &wgpu.image(), 1);
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
