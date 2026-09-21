use super::*;

fn context() -> Result<NativeContext, Box<dyn std::error::Error>> {
    #[cfg(feature = "dx12")]
    let backend = crate::NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = crate::NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = crate::NativeBackend::Metal;
    Ok(NativeContext::new(
        backend,
        &crate::NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: false,
        },
    )?)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn owned_resize_reuses_capacity_and_crops_readback_and_external_copy()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    let mut renderer = NativeRenderer::with_context(&context, 64, 48)?;
    let root = crate::RetainedNodeId::for_owner(1);
    let mut scene = crate::RetainedScene::new(64, 48, 1.0, root)?;
    let mut canvas = Canvas::new(96, 72, 1.0);
    canvas.push_rect(
        peniko::kurbo::Rect::new(2.5, 3.5, 92.5, 69.5),
        crate::Radius::ZERO,
        peniko::Color::from_rgba8(201, 31, 77, 197),
    );
    scene
        .transaction()
        .insert_scene(
            crate::RetainedParent::content(root),
            None,
            crate::RetainedNodeId::for_owner(2),
            Rc::new(canvas),
            peniko::kurbo::Affine::IDENTITY,
        )
        .commit()?;
    let mut previous: Option<crate::NativeTexture> = None;
    for (phase, (width, height)) in [(64, 48), (60, 44), (64, 48), (96, 72), (90, 68), (12, 10)]
        .into_iter()
        .enumerate()
    {
        scene.transaction().resize(width, height, 1.0).commit()?;
        renderer.render_retained(&scene)?.wait()?;
        let allocation = renderer.target.as_ref().unwrap();
        if let Some(previous) = &previous {
            assert_eq!(
                Rc::ptr_eq(&previous.state, &allocation.state),
                matches!(phase, 1 | 2 | 4),
                "nearby resize must reuse capacity; growth and large shrink must replace it"
            );
        }
        previous = Some(allocation.clone());
        let image = renderer.render_retained_to_image(&scene)?.readback()?;
        assert_eq!((image.width, image.height), (width, height));
        let mut fresh = NativeRenderer::with_context(&context, width, height)?;
        assert_eq!(
            image.pixels,
            fresh.render_retained_to_image(&scene)?.readback()?.pixels
        );
        let external = context.create_texture(width + 3, height + 4)?;
        renderer
            .render_retained_to_target(
                &scene,
                crate::NativeRenderTarget::transient(&external).with_origin(2, 3),
            )?
            .wait()?;
        let output = external.readback()?.readback()?;
        for y in 0..height + 4 {
            for x in 0..width + 3 {
                let expected = if x >= 2 && x < width + 2 && y >= 3 && y < height + 3 {
                    image.pixels[((y - 3) * width + x - 2) as usize]
                } else {
                    0
                };
                assert_eq!(output.pixels[(y * (width + 3) + x) as usize], expected);
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn completed_unchanged_owned_frames_need_no_submission() -> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    let mut renderer = NativeRenderer::with_context(&context, 16, 16)?;
    let scene = crate::RetainedScene::new(16, 16, 1.0, crate::RetainedNodeId::for_owner(1))?;
    let first = renderer.render_retained(&scene)?;
    // Without an observed completion, a no-damage receipt must still wait for
    // the earlier queued writes. GPU speed alone cannot justify skipping this.
    let second = renderer.render_retained(&scene)?;
    assert_eq!(renderer.incremental_render_stats().queue_submissions, 1);
    second.wait()?;
    first.wait()?;
    let idle = renderer.render_retained(&scene)?;
    assert_eq!(renderer.incremental_render_stats().queue_submissions, 0);
    assert!(idle.is_complete()?);
    idle.wait()?;
    // Readback and external output still have real work even with no damage.
    let image = renderer.render_retained_to_image(&scene)?.readback()?;
    assert_eq!(renderer.incremental_render_stats().queue_submissions, 1);
    assert!(image.pixels.iter().all(|pixel| *pixel == 0));
    let target = context.create_texture(16, 16)?;
    renderer
        .render_retained_to_texture(&scene, &target)?
        .wait()?;
    assert_eq!(renderer.incremental_render_stats().queue_submissions, 1);
    renderer.set_clear_color(peniko::Color::from_rgb8(200, 30, 60));
    renderer.render_retained(&scene)?.wait()?;
    assert_eq!(renderer.incremental_render_stats().queue_submissions, 1);
    Ok(())
}
