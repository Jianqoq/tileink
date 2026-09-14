use super::common::*;

#[test]
fn cache_pressure_preserves_nonuniform_backdrop_halo_before_later_foreground() {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(928_000);
    let earlier = RetainedNodeId::for_owner(928_002);
    let earlier_child = RetainedNodeId::for_owner(928_003);
    let leaf = |rect, color| {
        let mut canvas = Canvas::new(256, 256, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let region = |rect| Region::Rect {
        rect,
        radius: crate::Radius::ZERO,
    };
    let filter = |rect| RetainedLayerDescriptor::Filter {
        filter: Filter::Invert(1.0),
        sample_region: region(rect),
    };
    // Root-cause regression: a growing earlier filter must not evict unvisited
    // backdrop input during a partial frame. The green stripe lies outside the
    // dirty tiles; the later yellow stripe must never become its blur input.
    // A uniform background would hide a missing halo through source-over.
    let small = Rect::new(64.0, 64.0, 80.0, 80.0);
    let grown = Rect::new(64.0, 64.0, 96.0, 96.0);
    let mut scene = RetainedScene::new(256, 256, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(928_001),
            leaf(
                Rect::new(0.0, 0.0, 256.0, 256.0),
                Color::from_rgb8(0, 0, 255),
            ),
            Affine::IDENTITY,
        )
        .insert_layer(RetainedParent::content(root), None, earlier, filter(small))
        .insert_scene(
            RetainedParent::content(earlier),
            None,
            earlier_child,
            leaf(small, Color::from_rgb8(255, 0, 0)),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(928_006),
            leaf(
                Rect::new(112.0, 32.0, 116.0, 192.0),
                Color::from_rgb8(0, 255, 0),
            ),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(928_004),
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Blur {
                    std_dev_x: 4.0,
                    std_dev_y: 4.0,
                    sampling: Default::default(),
                },
                sample_region: region(Rect::new(32.0, 32.0, 224.0, 224.0)),
            },
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(928_005),
            leaf(
                Rect::new(112.0, 32.0, 116.0, 192.0),
                Color::from_rgb8(255, 255, 0),
            ),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut incremental = new_test_renderer(256, 256, Color::TRANSPARENT);
    let mut full = new_test_renderer(256, 256, Color::TRANSPARENT);
    let mut config = incremental.incremental_render_config();
    config.capture_active_tiles = true;
    // Exact first-frame fit: 16² filter output/input plus 256² backdrop output/input.
    // Growing the filter to 32² must not evict the not-yet-visited backdrop source.
    config.retained_texture_budget_bytes = 2 * (16 * 16 + 256 * 256) * 4;
    incremental.set_incremental_render_config(config);
    let mut full_config = full.incremental_render_config();
    full_config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(full_config);
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert_eq!(
        incremental
            .incremental_render_stats()
            .rerendered_offscreen_surfaces,
        2
    );
    assert_eq!(incremental.image().pixels, full.image().pixels);

    scene
        .transaction()
        .update_layer(earlier, filter(grown))
        .replace_scene(earlier_child, leaf(grown, Color::from_rgb8(255, 0, 0)))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert!(
        !incremental.incremental_render_stats().full_redraw,
        "the regression must exercise a partial frame, not next-frame recovery"
    );
    let stats = incremental.incremental_render_stats();
    let tile_at =
        |x: u32, y: u32| (y / crate::TILE_SIZE) * stats.tiles_width + x / crate::TILE_SIZE;
    assert!(
        stats.active_tiles.contains(&tile_at(111, 80)),
        "the halo probe must be repainted"
    );
    assert!(
        !stats.active_tiles.contains(&tile_at(112, 80)),
        "the green source stripe must remain outside the partial root work"
    );
    assert_eq!(
        incremental
            .incremental_render_stats()
            .rerendered_offscreen_surfaces,
        2
    );
    let actual = incremental.image();
    let expected = full.image();
    let pixel = |pixels: &[u32], x: usize, y: usize| pixels[y * 256 + x].to_le_bytes();
    assert!(
        pixel(&expected.pixels, 111, 80)[1] > 0,
        "the clean pre-backdrop stripe must contribute a green blur halo"
    );
    assert_eq!(
        actual.pixels, expected.pixels,
        "budget pressure must preserve painter-order backdrop source in the same frame"
    );
}
