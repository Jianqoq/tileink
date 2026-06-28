use super::*;

fn pixel_ix(x: usize, y: usize, width: usize) -> usize {
    y * width + x
}

fn component_transfer_test_table() -> Box<crate::shared::layer::filter::ComponentTransferTable> {
    use crate::shared::layer::filter::{
        COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE,
    };

    let mut table = Box::new([0; COMPONENT_TRANSFER_TABLE_LEN]);
    for i in 0..256 {
        table[i] = i as u32;
        table[COMPONENT_TRANSFER_TABLE_SIZE + i] = 255 - i as u32;
        table[2 * COMPONENT_TRANSFER_TABLE_SIZE + i] = 255;
        table[3 * COMPONENT_TRANSFER_TABLE_SIZE + i] = (i / 2) as u32;
    }
    table
}

#[test]
fn filter_wgpu_applies_color_filter_to_offscreen_children_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 8.0, 16.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[8 * 16 + 4], rgba8_pack([0, 255, 255, 255]));
    assert_eq!(target[8 * 16 + 12], 0);
}

#[test]
fn filter_wgpu_isolates_opacity_layer_with_offscreen_child_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(16, 16);
    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    scene.push_opacity_layer(full.to_path(0.0), Affine::IDENTITY, 0.0, 0.5);
    scene.push_rect(full, Color::from_rgb8(0, 128, 0), FillRule::NonZero);
    scene.push_filter_layer(Filter::Opacity(1.0), Region::rect(full, Radius::all(0.0)));
    scene.push_rect(full, Color::from_rgb8(0, 0, 255), FillRule::NonZero);
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[8 * 16 + 8], rgba8_pack([0, 0, 128, 128]));
}

#[test]
fn filter_wgpu_isolates_blend_layer_with_offscreen_child_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(16, 16);
    let full = Rect::new(0.0, 0.0, 16.0, 16.0);
    scene.push_rect(full, Color::from_rgb8(128, 128, 128), FillRule::NonZero);
    scene.push_blend_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        Mix::Multiply,
        Compose::SrcOver,
    );
    scene.push_rect(full, Color::from_rgb8(255, 0, 0), FillRule::NonZero);
    scene.push_filter_layer(Filter::Opacity(1.0), Region::rect(full, Radius::all(0.0)));
    scene.push_rect(full, Color::from_rgb8(0, 255, 0), FillRule::NonZero);
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[8 * 16 + 4], rgba8_pack([0, 128, 0, 255]));
    assert_eq!(target[8 * 16 + 12], rgba8_pack([128, 128, 128, 255]));
}

#[test]
fn filter_wgpu_applies_outer_clip_stack_to_offscreen_output_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_clip_layer(
        Rect::new(0.0, 0.0, 8.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.0,
    );
    scene.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[8 * 16 + 4], rgba8_pack([0, 255, 255, 255]));
    assert_eq!(target[8 * 16 + 12], 0);
}

#[test]
fn filter_wgpu_applies_outer_opacity_stack_to_offscreen_output_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(16, 16);
    scene.push_opacity_layer(
        Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.0),
        Affine::IDENTITY,
        0.0,
        0.5,
    );
    scene.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[8 * 16 + 8], rgba8_pack([0, 128, 128, 128]));
}

#[test]
fn filter_wgpu_applies_outer_blend_stack_to_offscreen_output_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

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
    scene.push_filter_layer(
        Filter::Opacity(1.0),
        Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();
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
fn filter_wgpu_blur_outputs_expanded_bounds_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(96, 96);
    let sample_rect = Rect::new(32.0, 32.0, 64.0, 64.0);
    scene.push_filter_layer(
        Filter::Blur {
            radius_x: 4.0,
            radius_y: 4.0,
        },
        Region::rect(sample_rect, Radius::all(0.0)),
    );
    scene.push_rect(sample_rect, Color::from_rgb8(255, 0, 0), FillRule::NonZero);
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(96, 96, Color::WHITE);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let expanded_px = unpack_rgba8(target[48 * 96 + 28]);
    let far_px = unpack_rgba8(target[48 * 96 + 16]);

    assert_eq!(expanded_px[0], 255);
    assert!(
        expanded_px[1] < 245 && expanded_px[2] < 245,
        "expected blur outside sample region, got {expanded_px:?}"
    );
    assert_eq!(far_px, [255, 255, 255, 255]);
}

#[test]
fn filter_wgpu_applies_anisotropic_blur_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(3, 3);
    scene.push_filter_layer(
        Filter::Blur {
            radius_x: 1.0,
            radius_y: 0.0,
        },
        Region::rect(Rect::new(0.0, 0.0, 3.0, 3.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(1.0, 1.0, 2.0, 2.0),
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(3, 3, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert!(unpack_rgba8(target[pixel_ix(0, 1, 3)])[3] > 0);
    assert!(unpack_rgba8(target[pixel_ix(2, 1, 3)])[3] > 0);
    assert_eq!(target[pixel_ix(1, 0, 3)], 0);
    assert_eq!(target[pixel_ix(1, 2, 3)], 0);
}

#[test]
fn filter_wgpu_drop_shadow_offsets_alpha_and_preserves_source_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::DropShadow {
            offset_x: 2.0,
            offset_y: 1.0,
            radius: 0.0,
            brush: Brush::Solid(Color::BLACK),
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(2.0, 2.0, 3.0, 3.0),
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[pixel_ix(2, 2, 8)], rgba8_pack([255, 255, 255, 255]));
    assert_eq!(target[pixel_ix(4, 3, 8)], rgba8_pack([0, 0, 0, 255]));
    assert_eq!(target[pixel_ix(1, 1, 8)], 0);
}

#[test]
fn filter_wgpu_applies_filter_chain_in_order_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::Chain {
            filters: vec![Filter::Brightness(2.0), Filter::Invert(1.0)],
            fixed_region: false,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        Color::from_rgb8(32, 32, 32),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.render(&scene);
    let px = unpack_rgba8(renderer.target.read(renderer.client())[pixel_ix(4, 4, 8)]);

    assert!(
        px[0].abs_diff(191) <= 1 && px[1].abs_diff(191) <= 1 && px[2].abs_diff(191) <= 1,
        "expected brightness before invert, got {px:?}"
    );
}

#[test]
fn filter_wgpu_uploads_drop_shadow_brushes_inside_chain_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::Chain {
            filters: vec![
                Filter::DropShadow {
                    offset_x: 1.0,
                    offset_y: 0.0,
                    radius: 0.0,
                    brush: Brush::Solid(Color::BLACK),
                },
                Filter::DropShadow {
                    offset_x: 0.0,
                    offset_y: 1.0,
                    radius: 0.0,
                    brush: Brush::Solid(Color::from_rgb8(255, 0, 0)),
                },
            ],
            fixed_region: true,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(2.0, 2.0, 3.0, 3.0),
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    let data = renderer.filter_brushes.data.read(renderer.client());

    assert_eq!(data.len(), 2 * crate::cubecl::brush::GPU_BRUSH_U32_STRIDE);
    assert_eq!(data[0], crate::cubecl::brush::GPU_BRUSH_SOLID);
    assert_eq!(
        data[crate::cubecl::brush::GPU_BRUSH_U32_STRIDE],
        crate::cubecl::brush::GPU_BRUSH_SOLID
    );
}

#[test]
fn filter_wgpu_executes_filter_graph_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
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
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    assert_eq!(
        renderer.filter_brushes.data.read(renderer.client()).len(),
        crate::cubecl::brush::GPU_BRUSH_U32_STRIDE
    );
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[pixel_ix(2, 4, 8)], rgba8_pack([0, 0, 0, 255]));
    assert_eq!(target[pixel_ix(6, 4, 8)], rgba8_pack([0, 0, 255, 255]));
}

#[test]
fn filter_wgpu_merges_filter_graph_inputs_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
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
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[pixel_ix(2, 4, 8)], rgba8_pack([255, 0, 0, 255]));
    assert_eq!(target[pixel_ix(6, 4, 8)], 0);
}

#[test]
fn filter_wgpu_morphology_dilates_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::Morphology {
            radius_x: 1.0,
            radius_y: 1.0,
            operator: MorphologyOperator::Dilate,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(3.0, 3.0, 4.0, 4.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[pixel_ix(2, 3, 8)], rgba8_pack([255, 0, 0, 255]));
    assert_eq!(target[pixel_ix(4, 4, 8)], rgba8_pack([255, 0, 0, 255]));
    assert_eq!(target[pixel_ix(1, 3, 8)], 0);
}

#[test]
fn filter_wgpu_morphology_huge_erode_clears_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::Morphology {
            radius_x: 9999.0,
            radius_y: 9999.0,
            operator: MorphologyOperator::Erode,
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert!(target.iter().all(|pixel| *pixel == 0));
}

#[test]
fn filter_wgpu_offsets_filter_buffer_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::Offset { dx: 2.0, dy: 1.0 },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(1.0, 1.0, 2.0, 2.0),
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[pixel_ix(1, 1, 8)], 0);
    assert_eq!(target[pixel_ix(3, 2, 8)], rgba8_pack([255, 255, 255, 255]));
}

#[test]
fn filter_wgpu_applies_color_matrix_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::ColorMatrix([
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
        ]),
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.render(&scene);

    assert_eq!(
        renderer.target.read(renderer.client())[pixel_ix(4, 4, 8)],
        rgba8_pack([0, 0, 255, 255])
    );
}

#[test]
fn filter_wgpu_applies_component_transfer_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::ComponentTransfer(component_transfer_test_table()),
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        Color::from_rgba8(255, 128, 0, 128),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.render(&scene);

    assert_eq!(
        renderer.target.read(renderer.client())[pixel_ix(4, 4, 8)],
        rgba8_pack([64, 32, 64, 64])
    );
}

#[test]
fn filter_wgpu_applies_convolve_matrix_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(3, 1);
    scene.push_filter_layer(
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
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        Color::from_rgb8(10, 0, 0),
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        Color::from_rgb8(20, 0, 0),
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        Color::from_rgb8(40, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(3, 1, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[pixel_ix(0, 0, 3)], rgba8_pack([20, 0, 0, 255]));
    assert_eq!(target[pixel_ix(1, 0, 3)], rgba8_pack([40, 0, 0, 255]));
    assert_eq!(target[pixel_ix(2, 0, 3)], rgba8_pack([40, 0, 0, 255]));
}

#[test]
fn filter_wgpu_applies_diffuse_lighting_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(3, 1);
    scene.push_filter_layer(
        Filter::DiffuseLighting(DiffuseLighting {
            surface_scale: 1.0,
            diffuse_constant: 1.0,
            lighting_color: [1.0, 0.0, 0.0],
            light_source: LightSource::Distant {
                azimuth: 180.0,
                elevation: 0.0,
            },
        }),
        Region::rect(Rect::new(0.0, 0.0, 3.0, 1.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        Color::from_rgba8(0, 0, 0, 0),
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(1.0, 0.0, 2.0, 1.0),
        Color::from_rgba8(0, 0, 0, 128),
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(2.0, 0.0, 3.0, 1.0),
        Color::BLACK,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(3, 1, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let center = unpack_rgba8(target[pixel_ix(1, 0, 3)]);

    assert!(
        center[0].abs_diff(180) <= 1 && center[1] == 0 && center[2] == 0 && center[3] == 255,
        "expected red diffuse lighting at alpha slope center, got {center:?}"
    );
}

#[test]
fn filter_wgpu_applies_specular_lighting_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(1, 1);
    scene.push_filter_layer(
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
        Region::rect(Rect::new(0.0, 0.0, 1.0, 1.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        Color::BLACK,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(1, 1, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());

    assert_eq!(target[pixel_ix(0, 0, 1)], rgba8_pack([128, 64, 0, 128]));
}

#[test]
fn filter_wgpu_floods_with_uploaded_brush_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(8, 8);
    scene.push_filter_layer(
        Filter::Flood {
            brush: Brush::Solid(Color::from_rgba8(0, 255, 0, 128)),
        },
        Region::rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 1.0, 1.0),
        Color::from_rgb8(255, 0, 0),
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(8, 8, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    assert_eq!(
        renderer.filter_brushes.data.read(renderer.client())[0],
        crate::cubecl::brush::GPU_BRUSH_SOLID
    );
    renderer.render(&scene);

    assert_eq!(
        renderer.target.read(renderer.client())[pixel_ix(7, 7, 8)],
        rgba8_pack([0, 128, 0, 128])
    );
}

#[test]
fn filter_wgpu_drop_shadow_blurs_offset_alpha_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut scene = Scene::new(32, 32);
    scene.push_filter_layer(
        Filter::DropShadow {
            offset_x: 0.0,
            offset_y: 8.0,
            radius: 2.0,
            brush: Brush::Solid(Color::BLACK),
        },
        Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(8.0, 8.0, 16.0, 16.0),
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(32, 32, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let shadow_px = unpack_rgba8(target[25 * 32 + 12]);
    let source_px = unpack_rgba8(target[12 * 32 + 12]);
    let far_px = unpack_rgba8(target[31 * 32 + 12]);

    assert_eq!(source_px, [255, 255, 255, 255]);
    assert_eq!(shadow_px[0..3], [0, 0, 0]);
    assert!(
        shadow_px[3] > 0 && shadow_px[3] < 255,
        "expected blurred shadow edge, got {shadow_px:?}"
    );
    assert_eq!(far_px, [0, 0, 0, 0]);
}

#[test]
fn filter_wgpu_drop_shadow_samples_linear_gradient_brush_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let shadow = Gradient::new_linear((0.0, 0.0), (31.0, 0.0))
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let mut scene = Scene::new(32, 48);
    scene.push_filter_layer(
        Filter::DropShadow {
            offset_x: 0.0,
            offset_y: 16.0,
            radius: 0.0,
            brush: Brush::from_gradient(&shadow),
        },
        Region::rect(Rect::new(0.0, 0.0, 32.0, 48.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(32, 48, Color::TRANSPARENT);
    renderer.render(&scene);
    assert_eq!(renderer.filter_brushes.data.read(renderer.client())[0], 2);
    let payload = renderer.filter_brushes.payloads.read(renderer.client());
    assert_eq!(unpack_rgba8(payload[0]), [255, 0, 0, 255]);
    assert_eq!(
        *payload.last().map(|px| unpack_rgba8(*px)).as_ref().unwrap(),
        [0, 0, 255, 255]
    );
    let target = renderer.target.read(renderer.client());
    let left_shadow = unpack_rgba8(target[20 * 32 + 4]);
    let right_shadow = unpack_rgba8(target[20 * 32 + 27]);

    assert_eq!(left_shadow[3], 255);
    assert_eq!(right_shadow[3], 255);
    assert!(
        left_shadow[0] > left_shadow[2],
        "expected red side of gradient shadow, got {left_shadow:?}"
    );
    assert!(
        right_shadow[2] > right_shadow[0],
        "expected blue side of gradient shadow, got {right_shadow:?}"
    );
}

#[test]
fn filter_wgpu_drop_shadow_samples_pattern_brush_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let pattern = Brush::Pattern(PatternBrush {
        image: Arc::new(Image {
            width: 2,
            height: 1,
            pixels: vec![rgba8_pack([255, 0, 0, 255]), rgba8_pack([0, 0, 255, 255])],
        }),
        transform: IDENTITY_TRANSFORM,
        opacity: 255,
    });
    let mut scene = Scene::new(16, 48);
    scene.push_filter_layer(
        Filter::DropShadow {
            offset_x: 0.0,
            offset_y: 16.0,
            radius: 0.0,
            brush: pattern,
        },
        Region::rect(Rect::new(0.0, 0.0, 16.0, 48.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 48, Color::TRANSPARENT);
    renderer.render(&scene);
    assert_eq!(renderer.filter_brushes.data.read(renderer.client())[0], 6);
    assert_eq!(
        renderer.filter_brushes.payloads.read(renderer.client()),
        vec![rgba8_pack([255, 0, 0, 255]), rgba8_pack([0, 0, 255, 255])]
    );
    let target = renderer.target.read(renderer.client());

    assert_eq!(unpack_rgba8(target[20 * 16]), [255, 0, 0, 255]);
    assert_eq!(unpack_rgba8(target[20 * 16 + 1]), [0, 0, 255, 255]);
    assert_eq!(unpack_rgba8(target[20 * 16 + 2]), [255, 0, 0, 255]);
}

#[test]
fn filter_wgpu_drop_shadow_samples_radial_gradient_brush_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let shadow = Gradient::new_radial((16.0, 24.0), 10.0)
        .with_stops([Color::from_rgb8(255, 0, 0), Color::from_rgb8(0, 0, 255)]);
    let mut scene = Scene::new(32, 48);
    scene.push_filter_layer(
        Filter::DropShadow {
            offset_x: 0.0,
            offset_y: 16.0,
            radius: 0.0,
            brush: Brush::from_gradient(&shadow),
        },
        Region::rect(Rect::new(0.0, 0.0, 32.0, 48.0), Radius::all(0.0)),
    );
    scene.push_rect(
        Rect::new(0.0, 0.0, 32.0, 16.0),
        Color::WHITE,
        FillRule::NonZero,
    );
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(32, 48, Color::TRANSPARENT);
    renderer.render(&scene);
    let target = renderer.target.read(renderer.client());
    let center = unpack_rgba8(target[24 * 32 + 16]);
    let edge = unpack_rgba8(target[24 * 32 + 26]);

    assert!(
        center[0] > center[2],
        "expected red radial center, got {center:?}"
    );
    assert!(edge[2] > edge[0], "expected blue radial edge, got {edge:?}");
}

#[test]
fn filter_wgpu_rasterizes_path_region_mask_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }

    let mut triangle = BezPath::new();
    triangle.move_to((4.0, 4.0));
    triangle.line_to((12.0, 4.0));
    triangle.line_to((4.0, 12.0));
    triangle.close_path();
    let region = Region::path(triangle, Affine::IDENTITY, 0.0);
    let mut scene = Scene::new(16, 16);
    scene.push_backdrop_layer(Filter::Invert(1.0), region.clone());
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
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
    let mask = renderer.acquire_scratch();
    renderer.clear_buffer(mask, 0);
    renderer.build_region_mask(
        mask,
        &region,
        Some(0),
        crate::shared::bounds::Bounds::new(4, 4, 12, 12),
    );

    let CubeRenderTarget::Scratch(mask_ix) = mask else {
        unreachable!();
    };
    let pixels = renderer.scratch[mask_ix].read(renderer.client());
    assert_eq!(pixels[6 * 16 + 6], rgba8_pack([255, 255, 255, 255]));
    assert_eq!(pixels[10 * 16 + 10], 0);
}

#[test]
fn filter_wgpu_rasterizes_nonzero_path_region_mask_when_enabled() {
    if std::env::var("TILEINK_RUN_CUBECL_WGPU_TESTS").as_deref() != Ok("1") {
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
    let mut scene = Scene::new(16, 16);
    scene.push_backdrop_layer(Filter::Invert(1.0), region.clone());
    scene.pop_layer();

    let mut renderer = WgpuRenderer::new_default_device(16, 16, Color::TRANSPARENT);
    renderer.prepare_scene(&scene);
    let mask = renderer.acquire_scratch();
    renderer.clear_buffer(mask, 0);
    renderer.build_region_mask(
        mask,
        &region,
        Some(0),
        crate::shared::bounds::Bounds::new(4, 4, 12, 12),
    );

    let CubeRenderTarget::Scratch(mask_ix) = mask else {
        unreachable!();
    };
    let pixels = renderer.scratch[mask_ix].read(renderer.client());
    assert_eq!(pixels[6 * 16 + 6], rgba8_pack([255, 255, 255, 255]));
    assert_eq!(pixels[10 * 16 + 10], 0);
}
