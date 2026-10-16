use super::*;
use crate::{Brush, Canvas, NativeRenderer, Radius};
use peniko::{Color, kurbo::Rect};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_brush_switches_without_changing_execution_topology() -> Result<()> {
    let context = context()?;
    let program = coordinates()?;
    for clipped in [false, true] {
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        let mut fingerprint = None;
        for (frame, brush) in [
            Brush::Solid(Color::WHITE),
            program.brush(uniform(&[[0.01, 0.02, 0.0, 1.0]]))?.into(),
            program.brush(uniform(&[[0.02, 0.01, 0.0, 1.0]]))?.into(),
            Brush::Solid(Color::BLACK),
            program.brush(uniform(&[[0.01, 0.02, 0.0, 1.0]]))?.into(),
        ]
        .into_iter()
        .enumerate()
        {
            let mut canvas = Canvas::new(32, 32, 1.0);
            if clipped {
                canvas.push_clip_sdf_rect_layer(Rect::new(4.0, 4.0, 28.0, 28.0), Radius::ZERO);
            }
            canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO, brush);
            if clipped {
                canvas.pop_layer();
            }
            let key = canvas.execution_plan_fingerprint();
            assert_eq!(*fingerprint.get_or_insert(key), key);
            let actual = renderer.render_to_image(&canvas)?.readback()?.pixels;
            let mut fresh = NativeRenderer::with_context(&context, 32, 32)?;
            let expected = fresh.render_to_image(&canvas)?.readback()?.pixels;
            assert_eq!(actual, expected, "frame={frame}, clipped={clipped}");
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_brush_retry_after_compilation_failure_keeps_current_paint() -> Result<()> {
    let context = context()?;
    let program = coordinates()?;
    let invalid = ShaderProgram::new(
        program.sources.as_ref().clone(),
        "missing",
        ShaderInput::None,
        4,
    )?;
    let canvas = |brush: Brush| {
        let mut canvas = Canvas::new(16, 16, 1.0);
        canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, brush);
        canvas
    };
    let mut renderer = NativeRenderer::with_context(&context, 16, 16)?;
    let words = uniform(&[[0.01, 0.02, 0.0, 1.0]]);
    renderer
        .render_to_image(&canvas(Color::WHITE.into()))?
        .readback()?;
    assert!(
        renderer
            .render_to_image(&canvas(invalid.brush(words.clone())?.into()))
            .is_err()
    );
    for brush in [program.brush(words)?.into(), Color::BLACK.into()] {
        let canvas = canvas(brush);
        let actual = renderer.render_to_image(&canvas)?.readback()?.pixels;
        let mut fresh = NativeRenderer::with_context(&context, 16, 16)?;
        assert_eq!(actual, fresh.render_to_image(&canvas)?.readback()?.pixels);
    }
    context.check_validation()?;
    Ok(())
}
