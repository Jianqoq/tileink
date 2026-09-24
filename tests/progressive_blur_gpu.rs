//! Public-pipeline coverage. Run with TILEINK_NATIVE_GPU and --ignored.
use peniko::{
    Color,
    kurbo::{Affine, Point, Rect},
};
use std::rc::Rc;
use tileink::{
    Canvas, Filter, NativeBackend, NativeContext, NativeContextOptions, NativeRenderer,
    ProgressiveBlur, Radius, Region, RetainedLayerDescriptor, RetainedNodeId, RetainedParent,
    RetainedScene,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn context() -> Result<NativeContext> {
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    Ok(NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: false,
        },
    )?)
}

fn content(color: Color) -> Canvas {
    let mut canvas = Canvas::new(129, 97, 1.0);
    canvas
        .push_checkerboard(
            Rect::new(0.0, 0.0, 129.0, 97.0),
            4.0,
            Color::BLACK,
            Color::WHITE,
        )
        .unwrap();
    canvas.push_rect(Rect::new(32.0, 36.0, 48.0, 52.0), Radius::ZERO, color);
    canvas
}

#[test]
#[ignore = "requires pinned TILEINK_NATIVE_GPU"]
fn progressive_retained_dirty_and_clean_frames_match_fresh_render() -> Result<()> {
    let context = context()?;
    for backdrop in [false, true] {
        let root = RetainedNodeId::for_owner(921_000);
        let leaf = RetainedNodeId::for_owner(921_001);
        let layer = RetainedNodeId::for_owner(921_002);
        let mut scene = RetainedScene::new(129, 97, 1.0, root)?;
        let filter = Filter::ProgressiveBlur(ProgressiveBlur::new(
            Point::new(20.0, 10.0),
            Point::new(90.0, 80.0),
            4.0,
        ));
        let region = Region::rect(Rect::new(12.0, 12.0, 117.0, 85.0), Radius::all(8.0));
        let descriptor = if backdrop {
            RetainedLayerDescriptor::Backdrop {
                filter,
                sample_region: region,
            }
        } else {
            RetainedLayerDescriptor::Filter {
                filter,
                sample_region: region,
            }
        };
        let initial = Rc::new(content(Color::from_rgb8(200, 10, 30)));
        let mut tx = scene.transaction();
        if backdrop {
            tx.insert_scene(
                RetainedParent::content(root),
                None,
                leaf,
                initial,
                Affine::IDENTITY,
            )
            .insert_layer(RetainedParent::content(root), None, layer, descriptor);
        } else {
            tx.insert_layer(RetainedParent::content(root), None, layer, descriptor)
                .insert_scene(
                    RetainedParent::content(layer),
                    None,
                    leaf,
                    initial,
                    Affine::IDENTITY,
                );
        }
        tx.commit()?;
        let mut retained = NativeRenderer::with_context(&context, 129, 97)?;
        let target = context.create_texture(129, 97)?;
        for frame in 0..4 {
            if frame == 2 {
                scene
                    .transaction()
                    .replace_scene(leaf, Rc::new(content(Color::from_rgb8(10, 80, 240))))
                    .commit()?;
            }
            retained
                .render_retained_to_texture(&scene, &target)?
                .wait()?;
            let actual = target.readback()?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 129, 97)?;
            let expected = fresh.render_retained_to_image(&scene)?.readback()?;
            assert_eq!(
                actual.pixels, expected.pixels,
                "backdrop={backdrop}, frame={frame}"
            );
            if frame == 1 || frame == 3 {
                assert_eq!(retained.incremental_render_stats().dirty_tiles, 0);
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires pinned TILEINK_NATIVE_GPU"]
fn progressive_scale_and_offscreen_origin_preserve_gradient_coordinates() -> Result<()> {
    let context = context()?;
    let mut renderer = NativeRenderer::with_context(&context, 128, 96)?;
    let make = |logical_scale: f64| {
        let mut canvas = Canvas::new(
            (128.0 / logical_scale) as u32,
            (96.0 / logical_scale) as u32,
            logical_scale as f32,
        );
        let rect = Rect::new(
            -8.0 / logical_scale,
            12.0 / logical_scale,
            100.0 / logical_scale,
            80.0 / logical_scale,
        );
        canvas.push_filter_layer(
            Filter::ProgressiveBlur(ProgressiveBlur::new(
                Point::new(0.0, 20.0 / logical_scale),
                Point::new(0.0, 64.0 / logical_scale),
                4.0 / logical_scale as f32,
            )),
            Region::rect(rect, Radius::ZERO),
        );
        canvas
            .push_checkerboard(rect, 4.0 / logical_scale as f32, Color::BLACK, Color::WHITE)
            .unwrap();
        canvas.pop_layer();
        canvas
    };
    let a = renderer.render_to_image(&make(1.0))?.readback()?;
    let b = renderer.render_to_image(&make(2.0))?.readback()?;
    assert_eq!(a.pixels, b.pixels);
    // Above the ramp the alternating source remains sharp despite local-surface translation.
    assert_ne!(a.rgba8_at(32, 16), a.rgba8_at(36, 16));
    // Below it, the repeated checkerboard converges towards gray.
    let p = a.rgba8_at(32, 68);
    assert!((110..145).contains(&p[0]), "{p:?}");
    Ok(())
}

#[test]
#[ignore = "requires pinned TILEINK_NATIVE_GPU"]
fn progressive_reused_pyramid_ignores_spare_capacity() -> Result<()> {
    let context = context()?;
    let mut reused = NativeRenderer::with_context(&context, 513, 257)?;
    for (width, height, sigma) in [
        (513, 257, 32.0),
        (35, 19, 2.0),
        (1, 9, 8.0),
        (259, 131, 16.0),
    ] {
        let rect = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
        let mut canvas = Canvas::new(width, height, 1.0);
        canvas
            .push_checkerboard(
                rect,
                3.0,
                Color::from_rgba8(240, 80, 20, 170),
                Color::TRANSPARENT,
            )
            .unwrap();
        canvas.push_backdrop_layer(
            Filter::ProgressiveBlur(ProgressiveBlur::new(
                Point::new(0.0, 0.0),
                Point::new(f64::from(width), f64::from(height)),
                sigma,
            )),
            Region::rect(rect, Radius::ZERO),
        );
        canvas.pop_layer();
        let actual = reused.render_to_image(&canvas)?.readback()?;
        let mut fresh = NativeRenderer::with_context(&context, width, height)?;
        let expected = fresh.render_to_image(&canvas)?.readback()?;
        assert_eq!(
            actual.pixels, expected.pixels,
            "{width}x{height}, sigma={sigma}"
        );
    }
    Ok(())
}
