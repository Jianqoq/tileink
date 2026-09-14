use super::common::*;

fn assert_partial_cache_miss(filter: Filter, expected_flat: Option<[u8; 4]>) {
    if !run_wgpu_tests() {
        return;
    }
    let root = RetainedNodeId::for_owner(929_000);
    let filtered = RetainedNodeId::for_owner(929_002);
    let foreground = RetainedNodeId::for_owner(929_004);
    let leaf = |rect, color| {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let bounds = Rect::new(0.0, 0.0, 64.0, 64.0);
    let foreground_bounds = Rect::new(20.0, 20.0, 22.0, 22.0);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(929_001),
            leaf(bounds, Color::from_rgb8(0, 0, 255)),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            filtered,
            RetainedLayerDescriptor::Filter {
                filter,
                sample_region: Region::Rect {
                    rect: bounds,
                    radius: crate::Radius::ZERO,
                },
            },
        )
        .insert_scene(
            RetainedParent::content(filtered),
            None,
            RetainedNodeId::for_owner(929_003),
            leaf(bounds, Color::from_rgb8(255, 0, 0)),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            foreground,
            leaf(foreground_bounds, Color::BLACK),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut incremental = new_test_renderer(64, 64, Color::TRANSPARENT);
    let mut full = new_test_renderer(64, 64, Color::TRANSPARENT);
    let mut full_config = full.incremental_render_config();
    full_config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(full_config);
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    if let Some(expected) = expected_flat {
        assert_eq!(incremental.image().rgba8_at(16, 24), expected);
    }
    assert_eq!(incremental.image().pixels, full.image().pixels);
    // Ordinary-filter eviction is legal between frames and must not force a
    // complete root redraw. A cache miss still needs complete source dependencies.
    let mut config = incremental.incremental_render_config();
    config.retained_texture_budget_bytes = 0;
    incremental.set_incremental_render_config(config);
    scene
        .transaction()
        .replace_scene(foreground, leaf(foreground_bounds, Color::WHITE))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert!(!incremental.incremental_render_stats().full_redraw);
    assert_eq!(incremental.incremental_render_stats().dirty_tiles, 1);
    assert_eq!(
        incremental
            .incremental_render_stats()
            .rerendered_offscreen_surfaces,
        1
    );
    let actual = incremental.image();
    if let Some(expected) = expected_flat {
        assert_eq!(
            actual.rgba8_at(16, 24),
            expected,
            "the filter must read neighbours outside the dirty tile"
        );
    }
    assert_eq!(
        actual.pixels,
        full.image().pixels,
        "cache eviction must preserve all valid RGBA bytes"
    );
}

#[test]
fn partial_erosion_cache_miss_preserves_pixels_at_the_dirty_tile_edge() {
    assert_partial_cache_miss(
        Filter::Morphology {
            radius_x: 1.0,
            radius_y: 0.0,
            operator: crate::shared::layer::filter::MorphologyOperator::Erode,
        },
        Some([255, 0, 0, 255]),
    );
}

#[test]
fn partial_convolution_cache_miss_preserves_pixels_at_the_dirty_tile_edge() {
    use crate::shared::layer::filter::{ConvolveEdgeMode, ConvolveMatrix};
    assert_partial_cache_miss(
        Filter::ConvolveMatrix(ConvolveMatrix {
            columns: 3,
            rows: 1,
            target_x: 1,
            target_y: 0,
            data: vec![1.0; 3],
            divisor: 3.0,
            bias: 0.0,
            edge_mode: ConvolveEdgeMode::Duplicate,
            preserve_alpha: false,
        }),
        Some([255, 0, 0, 255]),
    );
}

#[test]
fn partial_diffuse_lighting_cache_miss_preserves_pixels_at_the_dirty_tile_edge() {
    use crate::shared::layer::filter::{DiffuseLighting, LightSource};
    assert_partial_cache_miss(
        Filter::DiffuseLighting(DiffuseLighting {
            surface_scale: 2.0,
            diffuse_constant: 0.5,
            lighting_color: [1.0; 3],
            light_source: LightSource::Distant {
                azimuth: 0.0,
                elevation: 45.0,
            },
        }),
        None,
    );
}

#[test]
fn partial_specular_lighting_cache_miss_preserves_pixels_at_the_dirty_tile_edge() {
    use crate::shared::layer::filter::{LightSource, SpecularLighting};
    assert_partial_cache_miss(
        Filter::SpecularLighting(SpecularLighting {
            surface_scale: 2.0,
            specular_constant: 0.5,
            specular_exponent: 4.0,
            lighting_color: [1.0; 3],
            light_source: LightSource::Distant {
                azimuth: 0.0,
                elevation: 45.0,
            },
        }),
        None,
    );
}

#[test]
fn retained_wrap_preserves_its_domain_and_propagates_opposite_edge_damage() {
    if !run_wgpu_tests() {
        return;
    }
    use crate::shared::layer::filter::{ConvolveEdgeMode, ConvolveMatrix};
    let root = RetainedNodeId::for_owner(929_100);
    let filtered = RetainedNodeId::for_owner(929_102);
    let edge = RetainedNodeId::for_owner(929_104);
    let foreground = RetainedNodeId::for_owner(929_105);
    let leaf = |rect, color| {
        let mut canvas = Canvas::new(128, 128, 1.0);
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        std::rc::Rc::new(canvas)
    };
    let region = Rect::new(16.0, 16.0, 48.0, 48.0);
    let edge_rect = Rect::new(47.0, 16.0, 48.0, 48.0);
    let foreground_rect = Rect::new(20.0, 20.0, 22.0, 22.0);
    let mut scene = RetainedScene::new(128, 128, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(929_101),
            leaf(Rect::new(0.0, 0.0, 128.0, 128.0), Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            filtered,
            RetainedLayerDescriptor::Filter {
                filter: Filter::ConvolveMatrix(ConvolveMatrix {
                    columns: 3,
                    rows: 1,
                    target_x: 1,
                    target_y: 0,
                    data: vec![0.0, 0.0, 1.0],
                    divisor: 1.0,
                    bias: 0.0,
                    edge_mode: ConvolveEdgeMode::Wrap,
                    preserve_alpha: false,
                }),
                sample_region: Region::Rect {
                    rect: region,
                    radius: crate::Radius::ZERO,
                },
            },
        )
        .insert_scene(
            RetainedParent::content(filtered),
            None,
            RetainedNodeId::for_owner(929_103),
            leaf(region, Color::from_rgb8(255, 0, 0)),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(filtered),
            None,
            edge,
            leaf(edge_rect, Color::from_rgb8(0, 0, 255)),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            foreground,
            leaf(foreground_rect, Color::BLACK),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut incremental = new_test_renderer(128, 128, Color::TRANSPARENT);
    let mut full = new_test_renderer(128, 128, Color::TRANSPARENT);
    let mut config = full.incremental_render_config();
    config.mode = crate::IncrementalRenderMode::ForceFull;
    full.set_incremental_render_config(config);
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert_eq!(incremental.image().rgba8_at(16, 24), [0, 0, 255, 255]);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    // Changing a late foreground tile cannot change the filter's wrap period.
    scene
        .transaction()
        .replace_scene(foreground, leaf(foreground_rect, Color::WHITE))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert!(!incremental.incremental_render_stats().full_redraw);
    assert_eq!(incremental.incremental_render_stats().dirty_tiles, 1);
    assert_eq!(incremental.image().rgba8_at(16, 24), [0, 0, 255, 255]);
    assert_eq!(incremental.image().pixels, full.image().pixels);
    // A changed right edge feeds the left edge. Its domain is four root tiles.
    scene
        .transaction()
        .replace_scene(edge, leaf(edge_rect, Color::from_rgb8(0, 255, 0)))
        .commit()
        .unwrap();
    incremental.render_retained(&scene);
    full.render_retained(&scene);
    assert!(!incremental.incremental_render_stats().full_redraw);
    assert_eq!(incremental.incremental_render_stats().dirty_tiles, 4);
    assert_eq!(incremental.image().rgba8_at(16, 24), [0, 255, 0, 255]);
    assert_eq!(incremental.image().pixels, full.image().pixels);
}

#[test]
fn wrapped_filter_reads_source_pixels_outside_the_root_canvas() {
    if !run_wgpu_tests() {
        return;
    }
    use crate::shared::layer::filter::{ConvolveEdgeMode, ConvolveMatrix};
    let root = RetainedNodeId::for_owner(929_200);
    let filtered = RetainedNodeId::for_owner(929_201);
    let mut child = Canvas::new(64, 64, 1.0);
    child.push_rect(
        Rect::new(-16.0, 0.0, -15.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            filtered,
            RetainedLayerDescriptor::Filter {
                filter: Filter::ConvolveMatrix(ConvolveMatrix {
                    columns: 3,
                    rows: 1,
                    target_x: 1,
                    target_y: 0,
                    data: vec![1.0, 0.0, 0.0],
                    divisor: 1.0,
                    bias: 0.0,
                    edge_mode: ConvolveEdgeMode::Wrap,
                    preserve_alpha: false,
                }),
                sample_region: Region::Rect {
                    rect: Rect::new(-16.0, 0.0, 48.0, 64.0),
                    radius: crate::Radius::ZERO,
                },
            },
        )
        .insert_scene(
            RetainedParent::content(filtered),
            None,
            RetainedNodeId::for_owner(929_202),
            std::rc::Rc::new(child),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    for force_full in [false, true] {
        let mut renderer = new_test_renderer(64, 64, Color::TRANSPARENT);
        if force_full {
            let mut config = renderer.incremental_render_config();
            config.mode = crate::IncrementalRenderMode::ForceFull;
            renderer.set_incremental_render_config(config);
        }
        renderer.render_retained(&scene);
        let actual = renderer.image();
        assert_eq!(
            actual.rgba8_at(47, 24),
            [0, 0, 255, 255],
            "the right edge wraps to the off-canvas left edge"
        );
        assert_eq!(actual.rgba8_at(46, 24), [0, 0, 0, 0]);
    }
}
