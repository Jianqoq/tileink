use super::*;

#[test]
fn wgpu_renderer_applies_filter_graph_displacement_map_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(8, 2, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 8, 2),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::from_rgba8(255, 0, 0, 128)),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: Some(FilterInput::Primitive(0)),
                    region: Bounds::new(0, 0, 8, 2),
                    kind: FilterPrimitiveKind::DisplacementMap(DisplacementMap {
                        scale_x: 4.0,
                        scale_y: 0.0,
                        x_channel: ColorChannel::R,
                        y_channel: ColorChannel::A,
                        linear_rgb: false,
                    }),
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 2.0), crate::Radius::ZERO),
    );
    for x in 0..8 {
        canvas.push_rect(
            Rect::new(x as f64, 0.0, x as f64 + 1.0, 2.0),
            crate::Radius::ZERO,
            Color::from_rgb8((x as u8 + 1) * 20, 0, 0),
        );
    }
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(&image, &expected, 0, "filter graph displacement map");
}

#[test]
fn wgpu_renderer_generates_filter_graph_turbulence_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(32, 24, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![FilterPrimitive {
                input: FilterInput::SourceGraphic,
                input2: None,
                region: Bounds::new(6, 5, 28, 20),
                kind: FilterPrimitiveKind::Turbulence(Turbulence {
                    stitch_tiles: true,
                    linear_rgb: true,
                    ..test_turbulence(TurbulenceKind::FractalNoise, -20, 4)
                }),
            }],
            fixed_region: true,
        },
        Region::rect(Rect::new(4.0, 3.0, 30.0, 22.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(4.0, 3.0, 30.0, 22.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(&image, &expected, 0, "filter graph turbulence");
}

#[test]
fn wgpu_renderer_filter_graph_turbulence_uses_surface_origin_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(80, 24, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![FilterPrimitive {
                input: FilterInput::SourceGraphic,
                input2: None,
                region: Bounds::new(40, 4, 72, 20),
                kind: FilterPrimitiveKind::Turbulence(Turbulence {
                    base_frequency_x: 0.09,
                    base_frequency_y: 0.13,
                    tile_x: 40.0,
                    tile_y: 4.0,
                    tile_width: 32.0,
                    tile_height: 16.0,
                    ..test_turbulence(TurbulenceKind::Turbulence, 5, 3)
                }),
            }],
            fixed_region: true,
        },
        Region::rect(Rect::new(40.0, 4.0, 72.0, 20.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(40.0, 4.0, 72.0, 20.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let expected = render_native_wgpu(&canvas);

    assert_images_near(
        &image,
        &expected,
        0,
        "filter graph turbulence surface origin",
    );
}

#[test]
fn wgpu_renderer_rasterizes_path_region_mask_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut triangle = BezPath::new();
    triangle.move_to((4.0, 4.0));
    triangle.line_to((12.0, 4.0));
    triangle.line_to((4.0, 12.0));
    triangle.close_path();
    let region = Region::path(triangle, Affine::IDENTITY, 0.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_backdrop_layer(Filter::Invert(1.0), region.clone());
    canvas.pop_layer();

    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&canvas);
    assert_eq!(
        renderer
            .filter_paths
            .range_starts
            .read::<u32>(renderer.device(), renderer.queue(), 1),
        vec![0]
    );
    assert_eq!(
        renderer
            .filter_paths
            .range_ends
            .read::<u32>(renderer.device(), renderer.queue(), 1),
        vec![3]
    );
    assert_eq!(
        renderer
            .filter_paths
            .p0x
            .read::<i32>(renderer.device(), renderer.queue(), 3),
        vec![1024, 3072, 1024]
    );

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
