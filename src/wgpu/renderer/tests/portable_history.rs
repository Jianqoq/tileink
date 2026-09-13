use super::*;

#[test]
fn portable_partial_offscreen_frame_preserves_history_outside_active_tiles() {
    if !run_wgpu_tests() {
        return;
    }
    let Some((device, queue)) = shared_wgpu_test_device(true) else {
        return;
    };
    const SIZE: (u32, u32) = (129, 65);
    let root = RetainedNodeId::for_owner(884_000);
    let changing = RetainedNodeId::for_owner(884_001);
    let stable = RetainedNodeId::for_owner(884_002);
    let leaf = |rect, color| {
        let mut canvas = Canvas::new(SIZE.0, SIZE.1, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let mut background = Canvas::new(SIZE.0, SIZE.1, 1.0);
    background.push_rect(
        Rect::new(0.0, 0.0, 129.0, 65.0),
        crate::Radius::ZERO,
        Color::from_rgb8(40, 80, 120),
    );
    let filtered = Rect::new(96.0, 32.0, 126.0, 62.0);
    background.push_filter_layer(
        Filter::Invert(1.0),
        Region::rect(filtered, crate::Radius::ZERO),
    );
    background.push_rect(filtered, crate::Radius::ZERO, Color::from_rgb8(0, 0, 255));
    background.pop_layer();
    let mut scene = RetainedScene::new(SIZE.0, SIZE.1, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            stable,
            std::rc::Rc::new(background),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            changing,
            leaf(
                Rect::new(17.0, 17.0, 40.0, 31.0),
                Color::from_rgb8(255, 0, 0),
            ),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut incremental = Renderer::new(device, queue, SIZE.0, SIZE.1, Color::TRANSPARENT);
    let mut full = Renderer::new(device, queue, SIZE.0, SIZE.1, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert_eq!(incremental.image().pixels, full.image().pixels);

    for (rect, color) in [
        (
            Rect::new(17.0, 17.0, 40.0, 31.0),
            Color::from_rgb8(0, 255, 0),
        ),
        (
            Rect::new(121.0, 57.0, 129.0, 65.0),
            Color::from_rgb8(0, 0, 255),
        ),
    ] {
        // Scratch texture contents are unspecified between frames. Poisoning them
        // checks that untouched output comes from history, never a reused temporary.
        queue.write_texture(
            incremental.fine_portable_target.texture().as_image_copy(),
            &vec![0u8; (SIZE.0 * SIZE.1 * 4) as usize],
            ::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE.0 * 4),
                rows_per_image: Some(SIZE.1),
            },
            ::wgpu::Extent3d {
                width: SIZE.0,
                height: SIZE.1,
                depth_or_array_layers: 1,
            },
        );
        scene
            .transaction()
            .replace_scene(changing, leaf(rect, color))
            .commit()
            .unwrap();
        incremental.render_retained(&scene);
        full.render_retained(&scene);
        let stats = incremental.incremental_render_stats();
        assert!(!stats.full_redraw, "the small edit must remain incremental");
        let actual = incremental.image();
        let expected = full.image();
        let first = actual
            .pixels
            .iter()
            .zip(&expected.pixels)
            .enumerate()
            .find(|(_, (actual, expected))| actual != expected);
        assert_eq!(
            first, None,
            "offscreen plans must preserve inactive output pixels"
        );
    }
}
