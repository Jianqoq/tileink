use super::*;

#[test]
fn wgpu_renderer_rasterizes_nonzero_path_region_mask_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut path = BezPath::new();
    for _ in 0..2 {
        path.move_to((4.0, 4.0));
        path.line_to((12.0, 4.0));
        path.line_to((4.0, 12.0));
        path.close_path();
    }
    let region = Region::path(path, Affine::IDENTITY, 0.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_backdrop_layer(Filter::Invert(1.0), region.clone());
    canvas.pop_layer();

    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&canvas);
    let mask = renderer.acquire_scratch().expect("scratch mask");
    let mut commands = WgpuCommandBatch::new(renderer.device(), renderer.queue(), "test mask");
    renderer.clear_render_target(&mut commands, mask, 0);
    assert!(renderer.build_region_mask(
        &mut commands,
        mask,
        &region,
        Some(0),
        Bounds::new(4, 4, 12, 12)
    ));
    commands.finish();

    let pixels = read_render_target_u32(&renderer, mask, 16 * 16);
    assert_eq!(pixels[6 * 16 + 6], 0xffffffff);
    assert_eq!(pixels[10 * 16 + 10], 0);
}

#[test]
fn wgpu_renderer_rect_liquid_glass_backdrop_is_stable_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(64, 32, 1.0);
    for x in 0..64 {
        let v = (x * 4) as u8;
        canvas.push_rect(
            Rect::new(f64::from(x), 0.0, f64::from(x + 1), 32.0),
            crate::Radius::ZERO,
            Color::from_rgb8(v, v, v),
        );
    }
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 6,
            blur_sampling: BlurSampling::downsampled(2),
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(16.0, 4.0, 48.0, 28.0), crate::Radius::all(6.0)),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(&image, &expected, 0, "rect liquid glass backdrop");
}

#[test]
fn wgpu_renderer_clips_rect_liquid_glass_backdrop_with_outer_sdf_clip_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(20, 40, 80),
    );
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::all(6.0));
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 0,
            tint: Color::from_rgba8(255, 0, 0, 255),
            refraction_factor: 0.0,
            fresnel_factor: 0.0,
            glare_factor: 0.0,
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.pop_layer();
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_ne!(image.rgba8_at(8, 8), [20, 40, 80, 255]);
    assert_eq!(image.rgba8_at(0, 0), [20, 40, 80, 255]);
}

#[test]
fn wgpu_renderer_clips_rect_liquid_glass_children_with_outer_sdf_clip_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(20, 40, 80),
    );
    canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::all(6.0));
    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 0,
            tint: Color::from_rgba8(255, 0, 0, 255),
            refraction_factor: 0.0,
            fresnel_factor: 0.0,
            glare_factor: 0.0,
            ..RectLiquidGlass::default()
        }),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    canvas.pop_layer();
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
    assert_eq!(image.rgba8_at(0, 0), [20, 40, 80, 255]);
}

#[test]
fn wgpu_renderer_applies_affine_to_sdf_clip_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(64, 64, 1.0);
    let transform = Affine::translate((32.0, 32.0))
        * Affine::rotate(std::f64::consts::FRAC_PI_2)
        * Affine::scale_non_uniform(1.0, 1.5);
    assert!(canvas.push_clip_sdf_layer_transformed(
        crate::Sdf::Rect(crate::SdfRect {
            start: peniko::kurbo::Point::new(4.0, 4.0),
            end: peniko::kurbo::Point::new(20.0, 12.0),
            radius: crate::Radius::ZERO,
        }),
        transform,
    ));
    canvas.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(30, 210, 70),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let center = transform * peniko::kurbo::Point::new(12.0, 8.0);
    assert_eq!(
        image.rgba8_at(center.x as u32, center.y as u32),
        [30, 210, 70, 255]
    );
    assert_eq!(image.rgba8_at(40, 44), [0, 0, 0, 0]);
    assert_eq!(image.rgba8_at(24, 24), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_merges_filter_graph_inputs_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(8, 8, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::canvas(8, 8),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 4, 8),
                    kind: FilterPrimitiveKind::Merge {
                        inputs: vec![FilterInput::Primitive(0), FilterInput::SourceGraphic],
                    },
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(2, 4), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(6, 4), [0, 0, 0, 0]);
}
