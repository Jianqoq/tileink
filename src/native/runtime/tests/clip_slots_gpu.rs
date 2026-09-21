use crate::{Canvas, NativeContext, NativeContextOptions, NativeRenderer};
use peniko::kurbo::{Affine, Circle, Rect, Shape};

fn canvas(normal_schedule: bool) -> Canvas {
    let mut canvas = Canvas::new(96, 96, 1.0);
    // The second circle rejects a corner tile whose slots still contain the
    // first circle's particles. It must terminate that stream before fine reads it.
    for (center, radius, color) in [
        (
            (40.0, 40.0),
            30.0,
            peniko::Color::from_rgba8(230, 50, 10, 211),
        ),
        (
            (48.0, 48.0),
            20.0,
            peniko::Color::from_rgba8(10, 50, 230, 199),
        ),
    ] {
        canvas.push_clip_layer(
            Circle::new(center, radius).to_path(0.1),
            Affine::IDENTITY,
            crate::FillRule::NonZero,
            0.1,
        );
        canvas.push_rect(Rect::new(0.0, 0.0, 96.0, 96.0), crate::Radius::ZERO, color);
        canvas.pop_layer();
    }
    if normal_schedule {
        // An ordinary transparent batch preserves pixels while independently
        // forcing count/prefix/emit instead of reusing preallocated headers.
        canvas.push_rect(
            Rect::new(0.0, 0.0, 96.0, 96.0),
            crate::Radius::ZERO,
            peniko::Color::TRANSPARENT,
        );
    }
    canvas
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_reused_clip_slots_terminate_rejected_tiles() -> super::Result<()> {
    #[cfg(feature = "dx12")]
    // SAFETY: single-threaded GPU tests enable validation before creating devices.
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let context = NativeContext::new(
        super::backend(),
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let mut renderer = NativeRenderer::with_context(&context, 96, 96)?;
    let normal = canvas(true);
    let clipped = canvas(false);
    let expected = renderer.render_to_image(&normal)?.readback()?;
    for scene in [&clipped, &normal, &clipped] {
        assert_eq!(
            renderer.render_to_image(scene)?.readback()?.pixels,
            expected.pixels
        );
    }
    context.check_validation()?;
    Ok(())
}
