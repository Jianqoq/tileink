use super::common::*;
use peniko::kurbo::Shape;

fn solid_transfer(color: [u32; 4]) -> Filter {
    let mut table = Box::new([0; COMPONENT_TRANSFER_TABLE_LEN]);
    for (channel, value) in color.into_iter().enumerate() {
        table[channel * COMPONENT_TRANSFER_TABLE_SIZE
            ..(channel + 1) * COMPONENT_TRANSFER_TABLE_SIZE]
            .fill(value);
    }
    Filter::ComponentTransfer(table)
}

#[test]
fn nested_full_canvas_filters_select_their_own_transfer_tables() {
    if !run_wgpu_tests() {
        return;
    }
    let rect = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_opacity_layer(rect.to_path(0.1), Affine::IDENTITY, 0.1, 0.5);
    for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        canvas.push_filter_layer(
            solid_transfer(color),
            Region::rect(rect, crate::Radius::ZERO),
        );
        canvas.push_rect(rect, crate::Radius::ZERO, Color::WHITE);
        canvas.pop_layer();
    }
    canvas.pop_layer();
    let plan = canvas.compile(canvas.root_commands);
    assert!(matches!(&plan.ops[0], ExecOp::OffscreenLayer { children, .. } if children.len() == 2));
    let image = render_native_wgpu(&canvas);
    // Each local upload contains the entire root table sequence. The second
    // sibling must retain its root offset even though it renders into scratch.
    assert_eq!(image.rgba8_at(8, 8), [0, 0, 128, 128]);
}

#[test]
fn full_canvas_filter_sample_path_does_not_replace_child_mask_path() {
    if !run_wgpu_tests() {
        return;
    }
    let rect = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut triangle = BezPath::new();
    triangle.move_to((0.0, 0.0));
    triangle.line_to((16.0, 0.0));
    triangle.line_to((0.0, 16.0));
    triangle.close_path();
    let mut mask = Canvas::new(16, 16, 1.0);
    mask.push_rect(rect, crate::Radius::ZERO, Color::WHITE);
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_filter_layer(
        Filter::Offset { dx: 0.0, dy: 0.0 },
        Region::path(rect.to_path(0.1), Affine::IDENTITY, 0.1),
    );
    canvas.push_mask_layer(
        mask,
        Mask {
            region: Region::path(triangle, Affine::IDENTITY, 0.1),
            kind: MaskKind::Alpha,
        },
    );
    canvas.push_rect(rect, crate::Radius::ZERO, Color::WHITE);
    canvas.pop_layer();
    canvas.pop_layer();
    let image = render_native_wgpu(&canvas);
    assert_eq!(image.rgba8_at(2, 2), [255; 4]);
    // The sample rectangle occupies path slot 0; the triangular mask is slot 1.
    assert_eq!(image.rgba8_at(12, 12), [0; 4]);
}

#[test]
fn retained_root_filter_halo_preserves_surrounding_draws() {
    if !run_wgpu_tests() {
        return;
    }
    let rect = Rect::new(0.0, 0.0, 64.0, 64.0);
    let leaf = |rect, color| {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    // The zero-outset control retains the same root resource and worklist layout.
    for offset in [0.0, 2.0] {
        let root = RetainedNodeId::for_owner(918_000);
        let layer = RetainedNodeId::for_owner(918_002);
        let filter = Filter::Offset {
            dx: offset,
            dy: 0.0,
        };
        let region = Region::rect(
            Rect::new(
                offset as f64,
                offset as f64,
                64.0 - offset as f64,
                64.0 - offset as f64,
            ),
            crate::Radius::ZERO,
        );
        let surface = crate::shared::layer::filter::filter_surface_bounds(
            &filter,
            &region,
            Bounds::canvas(64, 64),
        )
        .unwrap();
        assert_eq!(surface.surface, Bounds::canvas(64, 64));
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(918_001),
                leaf(rect, Color::from_rgb8(0, 0, 255)),
                Affine::IDENTITY,
            )
            .insert_layer(
                RetainedParent::content(root),
                None,
                layer,
                RetainedLayerDescriptor::Filter {
                    filter,
                    sample_region: region,
                },
            )
            .insert_scene(
                RetainedParent::content(layer),
                None,
                RetainedNodeId::for_owner(918_003),
                leaf(rect, Color::from_rgb8(255, 0, 0)),
                Affine::IDENTITY,
            )
            .insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(918_004),
                leaf(
                    Rect::new(52.0, 52.0, 60.0, 60.0),
                    Color::from_rgb8(0, 255, 0),
                ),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        let mut incremental = new_test_renderer(64, 64, Color::TRANSPARENT);
        let mut full = new_test_renderer(64, 64, Color::TRANSPARENT);
        let mut config = full.incremental_render_config();
        config.mode = crate::IncrementalRenderMode::ForceFull;
        full.set_incremental_render_config(config);
        incremental.render_retained(&scene);
        full.render_retained(&scene);
        assert_eq!(
            incremental.image().pixels,
            full.image().pixels,
            "initial offset={offset}"
        );
        scene
            .transaction()
            .invalidate_rect(Rect::new(56.0, 56.0, 57.0, 57.0))
            .commit()
            .unwrap();
        incremental.render_retained(&scene);
        full.render_retained(&scene);
        let stats = incremental.incremental_render_stats();
        assert!(!stats.full_redraw);
        assert_eq!(stats.dirty_tiles, 1);
        assert_eq!(stats.rerendered_offscreen_surfaces, 1);
        let actual = incremental.image();
        let expected = full.image();
        let first = actual
            .pixels
            .iter()
            .zip(&expected.pixels)
            .enumerate()
            .find(|(_, (a, b))| a != b);
        assert_eq!(
            first, None,
            "filter halo work must not overwrite surrounding root draw worklists; offset={offset}"
        );
    }
}

#[test]
fn unavailable_pointwise_pipeline_rejects_filter_and_group_recording() {
    if !run_wgpu_tests() {
        return;
    }
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    renderer.filter = None;
    let mut commands = WgpuCommandBatch::new(
        renderer.device(),
        renderer.queue(),
        "missing filter pipeline",
    );
    for filter in [
        Filter::Opacity(0.5),
        Filter::ColorMatrix([0.0; 20]),
        solid_transfer([255, 0, 0, 255]),
    ] {
        assert!(!renderer.apply_filter(
            &mut commands,
            RenderTargetId::Main,
            Bounds::canvas(16, 16),
            &filter,
            None,
            &mut crate::render::filter_resources::cursors::FilterCursors::default()
        ));
    }
    let mut adapter =
        super::super::execution_adapter::WgpuExecutionAdapter::new(&mut renderer, &mut commands);
    assert!(
        crate::render::groups::GroupAdapter::apply_opacity(
            &mut adapter,
            RenderTargetId::Main,
            Bounds::canvas(16, 16),
            0.5
        )
        .is_err()
    );
    commands.finish();
}
