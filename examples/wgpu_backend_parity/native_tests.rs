use super::{Result, common, native};
use std::rc::Rc;
use tileink::{Canvas, NativeBackend, NativeContext, NativeContextOptions};

#[test]
#[ignore = "requires pinned GPU and Vulkan validation; run with --ignored"]
fn native_capture_rejects_nested_sessions_and_recovers_after_callback_failure() -> Result<()> {
    native::initialize_validation(true)?;
    let inputs = Rc::new(common::capture::Inputs {
        fonts: common::fonts::Snapshot::system()?,
        svgs: Default::default(),
    });
    let names = ["empty"];
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        let context = NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
                validation: true,
            },
        )?;
        let error = common::capture::run_native(&context, inputs.clone(), &names, || {
            assert!(
                common::capture::run_native(&context, inputs.clone(), &names, || Ok(())).is_err()
            );
            Err("injected callback failure".into())
        });
        assert!(
            error
                .err()
                .expect("callback must fail")
                .to_string()
                .contains("injected callback failure")
        );
        assert!(common::capture::run_native(&context, inputs.clone(), &names, || Ok(())).is_err());
        let clear = peniko::Color::from_rgba8(199, 73, 11, 127);
        let captured = common::capture::run_native(&context, inputs.clone(), &names, || {
            common::render_to_png_wgpu_with("empty", 7, 3, clear, |renderer| {
                renderer.render(&Canvas::new(7, 3, 1.0))
            })
        })?;
        assert_eq!(
            captured.images["empty"].pixels,
            tileink::Image::new(7, 3, clear).pixels
        );
        context.check_validation()?;
    }
    Ok(())
}

#[test]
#[ignore = "requires pinned four-API GPU and validation"]
fn native_simple_glass_matches_shared_example_scene() -> Result<()> {
    native::initialize_validation(true)?;
    let luid = std::env::var("TILEINK_NATIVE_GPU")?;
    let scene = common::liquid_glass_fast_path::single_mode_scene(
        common::liquid_glass_fast_path::GlassMode::Simple,
    );
    let instance = super::probe::instance()?;
    let mut reference = None;
    for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
        let mut route = super::gpu::create(&instance, backend, false, Some(&luid))?;
        route.renderer.set_clear_color(peniko::Color::WHITE);
        route.renderer.render(&scene);
        let image = route.renderer.image();
        if let Some(reference) = &reference {
            let diff = super::pixels::compare(reference, &image)?;
            assert_eq!(diff.pixels, 0, "{}: {diff:?}", route.name);
        } else {
            reference = Some(image);
        }
    }
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        let context = NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some(luid.clone()),
                validation: true,
            },
        )?;
        let mut renderer = tileink::NativeRenderer::with_context(
            &context,
            scene.physical_width(),
            scene.physical_height(),
        )?;
        renderer.set_clear_color(peniko::Color::WHITE);
        let image = renderer.render_to_image(&scene)?.readback()?;
        let diff = super::pixels::compare(reference.as_ref().unwrap(), &image)?;
        assert_eq!(diff.pixels, 0, "{backend:?}: {diff:?}");
        context.check_validation()?;
    }
    Ok(())
}
