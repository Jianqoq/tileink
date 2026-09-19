use super::fine_fixture::routes;
use crate::native::runtime::Result;
use crate::{
    Canvas, NativeBackend, NativeContext, NativeContextOptions, NativeError, NativeRenderer, Radius,
};
use peniko::{Color, kurbo::Rect};

fn scene(width: u32, height: u32, color: Color) -> Canvas {
    let mut canvas = Canvas::new(width, height, 1.0);
    canvas.push_rect(
        Rect::new(1.0, 1.0, f64::from(width - 1), f64::from(height - 1)),
        Radius::ZERO,
        color,
    );
    canvas
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_public_completion_poll_preserves_queued_images_until_readback() -> Result<()> {
    // Continuous hosts must inspect progress without waiting or consuming an image.
    // Completing a later submission also proves completion of its earlier prefix.
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let canvas = scene(19, 13, Color::from_rgba8(73, 191, 37, 127));
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
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
fn four_api_public_persistent_targets_reject_foreign_devices_and_replace_frames() -> Result<()> {
    let routes = routes()?;
    let scenes = [
        scene(23, 17, Color::from_rgba8(71, 191, 33, 127)),
        Canvas::new(23, 17, 1.0),
    ];
    let expected = scenes
        .iter()
        .map(|canvas| routes.canvas_reference(canvas))
        .collect::<Result<Vec<_>>>()?;
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        let options = NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        };
        let context = NativeContext::new(backend, &options)?;
        let foreign = NativeContext::new(backend, &options)?;
        let target = context.create_texture(23, 17)?;
        let mut renderer = NativeRenderer::with_context(&context, 23, 17)?;
        let wrong_size = context.create_texture(17, 23)?;
        assert!(renderer.render_to_texture(&scenes[0], &wrong_size).is_err());
        assert!(
            renderer
                .render_to_texture(&scenes[0], &foreign.create_texture(23, 17)?)
                .is_err()
        );
        assert_eq!(context.adapter.pending_count(), 0);
        for (canvas, expected) in scenes.iter().zip(&expected) {
            let submitted = renderer.render_to_texture(canvas, &target)?;
            let image = target.readback()?.readback()?;
            assert_eq!(bytemuck::cast_slice::<_, u8>(&image.pixels), expected[0]);
            submitted.wait()?;
        }
        context.check_validation()?;
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_public_clear_color_changes_without_stale_background() -> Result<()> {
    let routes = routes()?;
    let contexts = [NativeBackend::Dx12, NativeBackend::Vulkan].map(|backend| {
        NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU").unwrap()),
                validation: true,
            },
        )
    });
    let mut renderers = Vec::new();
    for context in contexts {
        renderers.push(NativeRenderer::with_context(&context?, 1, 1)?);
    }
    let scenes = [
        super::recording::nested_scene([71, 203, 139, 127]),
        Canvas::new(13, 9, 1.0),
        scene(19, 11, Color::from_rgba8(71, 203, 139, 127)),
    ];
    for clear in [
        Color::WHITE,
        Color::from_rgba8(201, 93, 17, 111),
        Color::TRANSPARENT,
    ] {
        for canvas in &scenes {
            let expected = routes.canvas_clear_reference(canvas, clear)?;
            for renderer in &mut renderers {
                renderer.set_clear_color(clear);
                let image = renderer.render_to_image(canvas)?.readback()?;
                assert_eq!(bytemuck::cast_slice::<_, u8>(&image.pixels), expected);
                renderer.context().check_validation()?;
            }
        }
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_public_renderer_preserves_pixels_and_independent_context_clients() -> Result<()> {
    let routes = routes()?;
    let first_scene = scene(23, 17, Color::from_rgba8(201, 99, 33, 127));
    let second_scene = scene(31, 19, Color::from_rgba8(17, 213, 81, 191));
    let first_expected = routes.canvas_reference(&first_scene)?;
    let second_expected = routes.canvas_reference(&second_scene)?;
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        for validation in [false, true] {
            let context = NativeContext::new(
                backend,
                &NativeContextOptions {
                    physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
                    validation,
                },
            )?;
            let mut first = NativeRenderer::with_context(&context, 1, 1)?;
            let mut second = NativeRenderer::with_context(&context, 1, 1)?;
            assert!(NativeRenderer::with_context(&context, 0, 1).is_err());
            assert_eq!(context.adapter.pending_count(), 0);
            first.render(&first_scene)?.wait()?;
            assert_eq!(context.adapter.pending_count(), 0);
            let a = first.render_to_image(&first_scene)?;
            let b = second.render_to_image(&second_scene)?;
            assert_eq!(first.size(), (23, 17));
            assert_eq!(second.size(), (31, 19));
            assert_eq!(context.adapter.pending_count(), 2);
            drop(first);
            drop(second);
            // Reverse readback order must not mix independent renderers' resources.
            let b = b.readback()?;
            let a = a.readback()?;
            assert_eq!(bytemuck::cast_slice::<_, u8>(&a.pixels), first_expected[0]);
            assert_eq!(bytemuck::cast_slice::<_, u8>(&b.pixels), second_expected[0]);
            assert_eq!(context.adapter.pending_count(), 0);
            context.check_validation()?;

            // A receipt owns the device even after both the renderer and context drop.
            let mut renderer = NativeRenderer::with_context(&context, 1, 1)?;
            let image = renderer.render_to_image(&first_scene)?;
            drop(renderer);
            drop(context);
            assert_eq!(
                bytemuck::cast_slice::<_, u8>(&image.readback()?.pixels),
                first_expected[0]
            );
        }
        let error = NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some("not-an-adapter".into()),
                validation: false,
            },
        )
        .unwrap_err();
        assert!(
            matches!(error, NativeError::Initialization(_)),
            "explicit selection must not fall back"
        );
    }
    routes.validate()
}
