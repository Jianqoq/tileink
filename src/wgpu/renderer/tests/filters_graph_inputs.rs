use super::*;

#[test]
fn wgpu_renderer_tiles_filter_graph_input_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(6, 4, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![FilterPrimitive {
                input: FilterInput::SourceGraphic,
                input2: None,
                region: Bounds::new(0, 0, 6, 4),
                kind: FilterPrimitiveKind::Tile {
                    source_region: Bounds::new(1, 1, 3, 3),
                },
            }],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 6.0, 4.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(1.0, 1.0, 2.0, 2.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_rect(
        Rect::new(2.0, 1.0, 3.0, 2.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    canvas.push_rect(
        Rect::new(1.0, 2.0, 2.0, 3.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    canvas.push_rect(
        Rect::new(2.0, 2.0, 3.0, 3.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 255, 0),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(0, 0), [255, 255, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
    assert_eq!(image.rgba8_at(2, 0), [255, 255, 0, 255]);
    assert_eq!(image.rgba8_at(3, 1), [255, 0, 0, 255]);
}

#[test]
fn wgpu_renderer_displaces_filter_graph_input_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(3, 1, 1.0);
    canvas.push_filter_layer(
        Filter::Graph {
            primitives: vec![
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 3, 1),
                    kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                        brush: Brush::Solid(Color::WHITE),
                    })),
                },
                FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: Some(FilterInput::Primitive(0)),
                    region: Bounds::new(0, 0, 3, 1),
                    kind: FilterPrimitiveKind::DisplacementMap(DisplacementMap {
                        scale_x: 2.0,
                        scale_y: 0.0,
                        x_channel: ColorChannel::R,
                        y_channel: ColorChannel::A,
                        linear_rgb: false,
                    }),
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 255, 0),
    );
    canvas.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(0, 0), [0, 255, 0, 255]);
    assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
    assert_eq!(image.rgba8_at(2, 0), [0, 0, 0, 0]);
}

#[test]
fn wgpu_renderer_applies_solid_flood_filter_when_enabled() {
    if !run_wgpu_tests() {
        return;
    }

    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::Flood {
            brush: Brush::Solid(Color::from_rgba8(20, 40, 80, 128)),
        },
        Region::rect(Rect::new(4.0, 4.0, 12.0, 12.0), crate::Radius::ZERO),
    );
    canvas.pop_layer();

    let image = render_native_wgpu(&canvas);

    assert_eq!(image.rgba8_at(8, 8), [10, 20, 40, 128]);
    assert_eq!(image.rgba8_at(2, 8), [0, 0, 0, 0]);
}
