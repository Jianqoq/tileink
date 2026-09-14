use super::common::*;

fn assert_nested_backdrop_parameter_updates(rect: Rect) {
    let root = RetainedNodeId::for_owner(932_000);
    let outer = RetainedNodeId::for_owner(932_001);
    let source_filter = RetainedNodeId::for_owner(932_002);
    let source = RetainedNodeId::for_owner(932_003);
    let backdrop = RetainedNodeId::for_owner(932_004);
    let region = || Region::rect(rect, crate::Radius::ZERO);
    let descriptor = |amount| RetainedLayerDescriptor::Filter {
        filter: Filter::Invert(amount),
        sample_region: region(),
    };
    let mut child = Canvas::new(64, 64, 1.0);
    child.push_rect(rect, crate::Radius::ZERO, Color::from_rgb8(0, 0, 255));
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            outer,
            RetainedLayerDescriptor::Filter {
                filter: Filter::Offset { dx: 16.0, dy: 0.0 },
                sample_region: region(),
            },
        )
        .insert_layer(
            RetainedParent::content(outer),
            None,
            source_filter,
            descriptor(0.0),
        )
        .insert_scene(
            RetainedParent::content(source_filter),
            None,
            source,
            std::rc::Rc::new(child),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(outer),
            None,
            backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Invert(1.0),
                sample_region: region(),
            },
        )
        .commit()
        .unwrap();
    let mut incremental = new_test_renderer(64, 64, Color::TRANSPARENT);
    let mut full = new_test_renderer(64, 64, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    let x = (rect.x0 + 24.0) as u32;
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert_eq!(full.image().rgba8_at(x, 24), [255, 255, 0, 255]);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    for (amount, expected) in [(1.0, [0, 0, 255, 255]), (0.0, [255, 255, 0, 255])] {
        scene
            .transaction()
            .update_layer(source_filter, descriptor(amount))
            .commit()
            .unwrap();
        incremental.render_retained(&scene);
        full.render_retained(&scene);
        assert!(!incremental.incremental_render_stats().full_redraw);
        assert_eq!(full.image().rgba8_at(x, 24), expected);
        assert_eq!(
            incremental.image().pixels,
            full.image().pixels,
            "nested backdrop must consume the changed local input before outer Offset; amount={amount}"
        );
    }
}

#[test]
fn nested_backdrop_parameter_updates_match_full_before_outer_offset() {
    if run_wgpu_tests() {
        assert_nested_backdrop_parameter_updates(Rect::new(16.0, 16.0, 32.0, 32.0));
    }
}

#[test]
fn off_canvas_backdrop_parameter_updates_match_full_before_outer_offset() {
    if run_wgpu_tests() {
        assert_nested_backdrop_parameter_updates(Rect::new(-16.0, 16.0, 0.0, 32.0));
    }
}

#[test]
fn expanding_ancestor_clip_rebuilds_the_revealed_backdrop_input() {
    if !run_wgpu_tests() {
        return;
    }
    use peniko::kurbo::Shape;
    let root = RetainedNodeId::for_owner(936_000);
    let clip = RetainedNodeId::for_owner(936_001);
    let full_rect = Rect::new(0.0, 0.0, 32.0, 16.0);
    let descriptor = |rect: Rect| RetainedLayerDescriptor::ClipPath {
        path: rect.to_path(0.1),
        transform: Affine::IDENTITY,
        rule: crate::FillRule::NonZero,
        tolerance: 0.1,
    };
    let mut background = Canvas::new(64, 64, 1.0);
    background.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let mut child = Canvas::new(64, 64, 1.0);
    child.push_rect(full_rect, crate::Radius::ZERO, Color::from_rgb8(0, 0, 255));
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(936_004),
            std::rc::Rc::new(background),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            clip,
            descriptor(Rect::new(0.0, 0.0, 16.0, 16.0)),
        )
        .insert_scene(
            RetainedParent::content(clip),
            None,
            RetainedNodeId::for_owner(936_002),
            std::rc::Rc::new(child),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(clip),
            None,
            RetainedNodeId::for_owner(936_003),
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Invert(1.0),
                sample_region: Region::rect(full_rect, crate::Radius::ZERO),
            },
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
    assert_eq!(full.image().rgba8_at(8, 8), [255, 255, 0, 255]);
    assert_eq!(full.image().rgba8_at(24, 8), [255, 255, 255, 255]);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    scene
        .transaction()
        .update_layer(clip, descriptor(full_rect))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert!(!incremental.incremental_render_stats().full_redraw);
    assert_eq!(full.image().rgba8_at(24, 8), [255, 255, 0, 255]);
    assert_eq!(
        incremental.image().pixels,
        full.image().pixels,
        "clip expansion must refresh cached backdrop input before compositing newly revealed pixels"
    );
}

#[test]
fn reparented_backdrop_retains_dependencies_for_later_partial_updates() {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(937_000);
    let outer = RetainedNodeId::for_owner(937_001);
    let source = RetainedNodeId::for_owner(937_002);
    let backdrop = RetainedNodeId::for_owner(937_003);
    let rect = Rect::new(0.0, 0.0, 32.0, 32.0);
    let region = || Region::rect(rect, crate::Radius::ZERO);
    let content = |color| {
        let mut canvas = Canvas::new(128, 128, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let blue = Color::from_rgb8(0, 0, 255);
    let mut scene = RetainedScene::new(128, 128, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            outer,
            RetainedLayerDescriptor::Filter {
                filter: Filter::Invert(0.0),
                sample_region: region(),
            },
        )
        .insert_scene(
            RetainedParent::content(outer),
            None,
            source,
            content(blue),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Invert(1.0),
                sample_region: region(),
            },
        )
        .commit()
        .unwrap();
    let mut incremental = new_test_renderer(128, 128, Color::TRANSPARENT);
    let mut full = new_test_renderer(128, 128, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    let check = |incremental: &mut Renderer, full: &mut Renderer, scene: &RetainedScene| {
        incremental.render_retained(scene);
        full.render(&scene.to_canvas());
        assert_eq!(incremental.image().pixels, full.image().pixels);
    };
    check(&mut incremental, &mut full, &scene);
    // Root cause: reparent keeps the Layer generation, so its reused chunk must
    // retain dependency-index membership for subsequent local input revisions.
    scene
        .transaction()
        .reparent(backdrop, RetainedParent::content(outer), None)
        .commit()
        .unwrap();
    check(&mut incremental, &mut full, &scene);
    for (color, expected) in [
        (Color::from_rgb8(255, 0, 0), [0, 255, 255, 255]),
        (blue, [255, 255, 0, 255]),
    ] {
        scene
            .transaction()
            .replace_scene(source, content(color))
            .commit()
            .unwrap();
        check(&mut incremental, &mut full, &scene);
        assert_eq!(full.image().rgba8_at(8, 8), expected);
        assert!(!incremental.incremental_render_stats().full_redraw);
    }
}
