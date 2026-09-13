use super::common::*;
use crate::shared::layer::Layer;

fn triangular_region() -> Region {
    let mut path = BezPath::new();
    path.move_to((16.0, 16.0));
    path.line_to((48.0, 16.0));
    path.line_to((16.0, 48.0));
    path.close_path();
    Region::path(path, Affine::IDENTITY, 0.1)
}

#[test]
fn path_backdrop_without_outer_clip_renders_coverage_and_children() {
    if !run_wgpu_tests() {
        return;
    }
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    canvas.push_backdrop_layer(Filter::Invert(1.0), triangular_region());
    canvas.push_rect(
        Rect::new(52.0, 52.0, 56.0, 56.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();
    let plan = canvas.compile(canvas.root_commands);
    assert!(plan.ops.iter().any(|op| matches!(op, ExecOp::OffscreenLayer { layer: Layer::Backdrop { sample_region: Region::Path { .. }, .. }, outer_stack, .. } if outer_stack.is_empty())));
    let mut renderer = new_test_renderer(64, 64, Color::TRANSPARENT);
    renderer.prepare_scene(&canvas);
    assert!(
        renderer.render_prepared_tile_plan(&canvas),
        "a path backdrop without outer clips must render through its coverage mask"
    );
    let image = renderer.image();
    assert_eq!(image.rgba8_at(20, 20), [255, 255, 0, 255]);
    assert_eq!(image.rgba8_at(40, 40), [0, 0, 255, 255]);
    assert_eq!(image.rgba8_at(54, 54), [255, 0, 0, 255]);
}

#[test]
fn retained_path_backdrop_reuses_coverage_when_only_foreground_changes() {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(927_000);
    let layer = RetainedNodeId::for_owner(927_002);
    let foreground = RetainedNodeId::for_owner(927_003);
    let leaf = |rect, color| {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let foreground_rect = Rect::new(24.0, 24.0, 28.0, 28.0);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(927_001),
            leaf(Rect::new(0.0, 0.0, 64.0, 64.0), Color::from_rgb8(0, 0, 255)),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Invert(1.0),
                sample_region: triangular_region(),
            },
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            foreground,
            leaf(foreground_rect, Color::from_rgb8(255, 0, 0)),
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
        incremental.image().rgba8_at(20, 20),
        [255, 255, 0, 255],
        "the first cached path backdrop must render its coverage"
    );
    assert_eq!(incremental.image().pixels, full.image().pixels);
    scene
        .transaction()
        .replace_scene(foreground, leaf(foreground_rect, Color::WHITE))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert!(!incremental.incremental_render_stats().full_redraw);
    assert_eq!(
        incremental
            .incremental_render_stats()
            .reused_offscreen_surfaces,
        1
    );
    assert_eq!(
        incremental
            .incremental_render_stats()
            .rerendered_offscreen_surfaces,
        0
    );
    let actual = incremental.image();
    assert_eq!(actual.rgba8_at(26, 26), [255; 4]);
    assert_eq!(
        actual.pixels,
        full.image().pixels,
        "cached path coverage must survive a foreground-only update"
    );
}
