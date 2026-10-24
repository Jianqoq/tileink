use crate::{Canvas, NativeContext, NativeContextOptions, NativeRenderer};
use peniko::kurbo::{Affine, Circle, Rect, Shape};

fn canvas(normal_schedule: bool, scale: u32, small_second_clip: bool) -> Canvas {
    let size = 96 * scale;
    let scale = f64::from(scale);
    let mut canvas = Canvas::new(size, size, 1.0);
    // Overlapping circles exercise rejected-tile stream termination. A small
    // second circle also revisits a tile classified EMPTY by the scalar batch;
    // its parallel emitter must replace that classification.
    for (center, radius, color) in [
        (
            (40.0, 40.0),
            30.0,
            peniko::Color::from_rgba8(230, 50, 10, 211),
        ),
        (
            if small_second_clip {
                (12.0, 12.0)
            } else {
                (48.0, 48.0)
            },
            if small_second_clip { 3.0 } else { 20.0 },
            peniko::Color::from_rgba8(10, 50, 230, 199),
        ),
    ] {
        canvas.push_clip_layer(
            Circle::new((center.0 * scale, center.1 * scale), radius * scale).to_path(0.1),
            Affine::IDENTITY,
            crate::FillRule::NonZero,
            0.1,
        );
        canvas.push_rect(
            Rect::new(0.0, 0.0, f64::from(size), f64::from(size)),
            crate::Radius::ZERO,
            color,
        );
        canvas.pop_layer();
    }
    if normal_schedule {
        // An ordinary transparent batch preserves pixels while independently
        // forcing count/prefix/emit instead of reusing preallocated headers.
        canvas.push_rect(
            Rect::new(0.0, 0.0, f64::from(size), f64::from(size)),
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
    // Exercise both the small parallel and the larger scalar clip schedule.
    for (scale, small) in [(1, false), (4, false), (4, true)] {
        let normal = canvas(true, scale, small);
        let clipped = canvas(false, scale, small);
        let expected = renderer.render_to_image(&normal)?.readback()?;
        for scene in [&clipped, &normal, &clipped] {
            assert!(
                renderer.render_to_image(scene)?.readback()?.pixels == expected.pixels,
                "reused clip slots differ: scale={scale} small={small}"
            );
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_nested_clip_slots_match_regular_allocation() -> super::Result<()> {
    #[cfg(feature = "dx12")]
    // SAFETY: validation is enabled before device creation, in single-threaded tests.
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
    let mut renderer = NativeRenderer::with_context(&context, 640, 400)?;
    for depth in [1, 4, 8, 16, 32] {
        let mut clipped = Canvas::new(640, 400, 1.0);
        let rect = Rect::new(1.0, 0.0, 385.0, 240.0);
        for level in 0..depth {
            clipped.push_clip_layer(
                peniko::kurbo::RoundedRect::from_rect(
                    rect,
                    240.0 * (0.1 + f64::from(level % 3) * 0.05),
                )
                .to_path(0.1),
                Affine::IDENTITY,
                crate::FillRule::NonZero,
                0.1,
            );
        }
        clipped.push_rect(
            rect,
            crate::Radius::ZERO,
            peniko::Color::from_rgba8(40, 110, 190, 211),
        );
        for _ in 0..depth {
            clipped.pop_layer();
        }
        let mut regular = clipped.clone();
        regular.push_rect(rect, crate::Radius::ZERO, peniko::Color::TRANSPARENT);
        let expected = renderer.render_to_image(&regular)?.readback()?;
        let actual = renderer.render_to_image(&clipped)?.readback()?;
        assert_eq!(
            actual
                .pixels
                .iter()
                .zip(&expected.pixels)
                .filter(|(a, b)| a != b)
                .count(),
            0,
            "depth {depth}"
        );
    }
    context.check_validation()?;
    Ok(())
}
