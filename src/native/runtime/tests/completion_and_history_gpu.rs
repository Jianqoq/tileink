use super::super::Result;
use crate::{
    Canvas, NativeContext, NativeContextOptions, NativeRenderer, Radius, RetainedNodeId,
    RetainedParent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_public_completion_poll_preserves_queued_images_until_readback() -> Result<()> {
    // Continuous hosts must inspect progress without waiting or consuming an image.
    // Completing a later submission also proves completion of its earlier prefix.
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let mut canvas = Canvas::new(19, 13, 1.0);
    canvas.push_rect(
        Rect::new(1.0, 1.0, 18.0, 12.0),
        Radius::ZERO,
        Color::from_rgba8(73, 191, 37, 127),
    );
    for backend in [super::backend()] {
        let context = NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
                validation: true,
            },
        )?;
        let mut renderer = NativeRenderer::with_context(&context, 19, 13)?;
        let first = renderer.render_to_image(&canvas)?;
        let _ = first.is_complete()?;
        let last = renderer.render(&canvas)?;
        let _ = last.is_complete()?;
        assert_eq!(context.adapter.pending_count(), 2);
        last.wait()?;
        assert!(first.is_complete()?);
        assert!(first.is_complete()?);
        assert_eq!(context.adapter.pending_count(), 1);
        let image = first.readback()?;
        assert_eq!((image.width, image.height), (19, 13));
        assert_eq!(context.adapter.pending_count(), 0);
        context.check_validation()?;
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_retained_output_rectangles_preserve_sentinels_and_history_contracts() -> Result<()> {
    use crate::{ExternalTextureHistoryId, NativeRenderTarget};
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    for backend in [super::backend()] {
        let context = NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
                validation: true,
            },
        )?;
        let texture = context.create_texture(24, 20)?;
        let mut writer = NativeRenderer::with_context(&context, 24, 20)?;
        writer.set_clear_color(Color::from_rgba8(11, 29, 43, 255));
        writer
            .render_to_texture(&Canvas::new(24, 20, 1.0), &texture)?
            .wait()?;
        let root = RetainedNodeId::for_owner(910_000);
        let node = RetainedNodeId::for_owner(910_001);
        let mut scene = RetainedScene::new(16, 12, 1.0, root)?;
        let mut child = Canvas::new(16, 12, 1.0);
        child.push_rect(
            Rect::new(0.0, 0.0, 16.0, 12.0),
            Radius::ZERO,
            Color::from_rgba8(211, 37, 71, 255),
        );
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                node,
                Rc::new(child),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 16, 12)?;
        for transient in [false, true] {
            let target = if transient {
                NativeRenderTarget::transient(&texture)
            } else {
                NativeRenderTarget::persistent(&texture, ExternalTextureHistoryId::new(1))
            }
            .with_origin(3, 5);
            for frame in 0..2 {
                renderer.render_retained_to_target(&scene, target)?.wait()?;
                if frame == 1 {
                    assert_eq!(renderer.incremental_render_stats().dirty_tiles, 0);
                }
                let image = texture.readback()?.readback()?;
                for y in 0..20 {
                    for x in 0..24 {
                        let expected = if (3..19).contains(&x) && (5..17).contains(&y) {
                            [211, 37, 71, 255]
                        } else {
                            [11, 29, 43, 255]
                        };
                        assert_eq!(image.rgba8_at(x, y), expected, "{backend:?} at {x},{y}");
                    }
                }
                if transient {
                    writer
                        .render_to_texture(&Canvas::new(24, 20, 1.0), &texture)?
                        .wait()?;
                }
            }
        }
        let fresh = NativeRenderTarget::persistent(&texture, ExternalTextureHistoryId::new(2))
            .with_origin(3, 5);
        renderer.render_retained_to_target(&scene, fresh)?.wait()?;
        assert!(renderer.incremental_render_stats().full_redraw);
        assert!(
            renderer
                .render_retained_to_target(&scene, fresh.with_origin(u32::MAX, 5))
                .is_err()
        );
        renderer.render_retained_to_target(&scene, fresh)?.wait()?;
        assert_eq!(
            renderer.incremental_render_stats().dirty_tiles,
            0,
            "rejected rectangle must not alter valid history"
        );
        context.check_validation()?;
    }
    Ok(())
}
