use super::*;

#[test]
fn wgpu_renderer_applies_path_clip_in_tile_fine_when_enabled() {
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
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);

    renderer.render(&canvas);
    let image = renderer.image();

    assert_eq!(image.rgba8_at(4, 8), [255, 0, 0, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_opacity_layer_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_opacity_layer(
        Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        0.5,
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);

    renderer.render(&canvas);
    let pixel = renderer.image().rgba8_at(8, 8);

    assert!(
        (126..=129).contains(&pixel[3]),
        "unexpected pixel {pixel:?}"
    );
    assert_eq!(pixel[1], 0);
    assert_eq!(pixel[2], 0);
}

#[test]
fn wgpu_renderer_applies_blend_layer_in_tile_fine_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(200, 80, 40));
    canvas.push_blend_layer(
        full.to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    canvas.push_rect(full, crate::Radius::ZERO, Color::from_rgb8(64, 200, 180));
    canvas.pop_layer();

    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    renderer.render(&canvas);

    let expected = render_native_wgpu(&canvas);
    assert_images_near(&renderer.image(), &expected, 0, "multiply blend layer");
}

#[test]
fn wgpu_renderer_applies_color_filter_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 8.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(4, 8), [0, 255, 255, 255]);
    assert_eq!(image.rgba8_at(12, 8), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_color_matrix_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::ColorMatrix([
            0.0, 0.0, 0.0, 0.0, 0.0, //
            1.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, 0.0,
        ]),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_component_transfer_to_offscreen_children_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut table = Box::new([0; COMPONENT_TRANSFER_TABLE_LEN]);
    for i in 0..COMPONENT_TRANSFER_TABLE_SIZE {
        table[i] = 0;
        table[COMPONENT_TRANSFER_TABLE_SIZE + i] = if i == 0 { 255 } else { i as u32 };
        table[2 * COMPONENT_TRANSFER_TABLE_SIZE + i] = 0;
        table[3 * COMPONENT_TRANSFER_TABLE_SIZE + i] = i as u32;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::ComponentTransfer(table),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(8, 8), [0, 255, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_convolve_matrix_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(3, 1, 1.0);
    canvas.push_filter_layer(
        Filter::ConvolveMatrix(ConvolveMatrix {
            columns: 3,
            rows: 1,
            target_x: 1,
            target_y: 0,
            data: vec![1.0, 0.0, 0.0],
            divisor: 1.0,
            bias: 0.0,
            edge_mode: ConvolveEdgeMode::Duplicate,
            preserve_alpha: false,
        }),
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(10, 0, 0),
    );
    canvas.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(20, 0, 0),
    );
    canvas.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(40, 0, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(0, 0), [20, 0, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [40, 0, 0, 255]);
    assert_eq!(image.rgba8_at(2, 0), [40, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_applies_diffuse_lighting_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(3, 1, 1.0);
    canvas.push_filter_layer(
        Filter::DiffuseLighting(DiffuseLighting {
            surface_scale: 1.0,
            diffuse_constant: 1.0,
            lighting_color: [1.0, 0.0, 0.0],
            light_source: LightSource::Distant {
                azimuth: 180.0,
                elevation: 0.0,
            },
        }),
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgba8(0, 0, 0, 0),
    );
    canvas.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgba8(0, 0, 0, 128),
    );
    canvas.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);
    let center = image.rgba8_at(1, 0);

    assert!(
        center[0].abs_diff(180) <= 1 && center[1] == 0 && center[2] == 0 && center[3] == 255,
        "expected red diffuse lighting at alpha slope center, got {center:?}"
    );
}

#[test]
fn wgpu_renderer_applies_specular_lighting_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(1, 1, 1.0);
    canvas.push_filter_layer(
        Filter::SpecularLighting(SpecularLighting {
            surface_scale: 0.0,
            specular_constant: 0.5,
            specular_exponent: 1.0,
            lighting_color: [1.0, 0.5, 0.0],
            light_source: LightSource::Point {
                x: 0.5,
                y: 0.5,
                z: 1.0,
            },
        }),
        Region::rect(Rect::new(0.0, 0.0, 1.0, 1.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::BLACK,
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(0, 0), [128, 64, 0, 128]);
}

#[test]
fn wgpu_renderer_executes_filter_graph_when_enabled() {
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
                    input2: Some(FilterInput::Primitive(0)),
                    region: Bounds::new(0, 0, 4, 8),
                    kind: FilterPrimitiveKind::Blend {
                        mode: Mix::Multiply,
                    },
                },
                FilterPrimitive {
                    input: FilterInput::Primitive(0),
                    input2: Some(FilterInput::SourceAlpha),
                    region: Bounds::new(4, 0, 8, 8),
                    kind: FilterPrimitiveKind::Composite {
                        operator: CompositeOperator::In,
                    },
                },
                FilterPrimitive {
                    input: FilterInput::Primitive(1),
                    input2: Some(FilterInput::Primitive(2)),
                    region: Bounds::canvas(8, 8),
                    kind: FilterPrimitiveKind::Composite {
                        operator: CompositeOperator::Over,
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

    assert_eq!(image.rgba8_at(2, 4), [0, 0, 0, 255]);
    assert_eq!(image.rgba8_at(6, 4), [0, 0, 255, 255]);
}
