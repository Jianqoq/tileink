//! Native user shaders exercise the production compiler and renderer directly.
use super::{Result, brush_support::context};
use crate::{ShaderInput, ShaderProgram, ShaderSources};
#[path = "shader_pixels.rs"]
mod pixels;
#[path = "shader_sampling.rs"]
mod sampling;
fn pack(value: [f64; 4]) -> u32 {
    let a = value[3].clamp(0.0, 1.0);
    u32::from_le_bytes(value.map(|v| (v.clamp(0.0, a) * 255.0).round() as u8))
}
fn close(actual: u32, expected: u32) {
    assert!(
        actual
            .to_le_bytes()
            .iter()
            .zip(expected.to_le_bytes())
            .all(|(a, b)| a.abs_diff(b) <= 1),
        "{actual:08x} != {expected:08x}"
    );
}
fn uniform(values: &[[f32; 4]]) -> Vec<u32> {
    values.iter().flatten().map(|v| v.to_bits()).collect()
}
fn coordinates() -> Result<ShaderProgram> {
    Ok(ShaderProgram::new(
        ShaderSources {
            metal: Some(include_str!("../../../../examples/shaders/coordinates.metal").into()),
            hlsl: Some(include_str!("../../../../examples/shaders/coordinates.hlsl").into()),
        },
        "coordinates",
        ShaderInput::None,
        4,
    )?)
}
fn input_scale() -> Result<ShaderProgram> {
    Ok(ShaderProgram::new(
        ShaderSources {
            metal: Some(include_str!("../../../../examples/shaders/scale.metal").into()),
            hlsl: Some(include_str!("../../../../examples/shaders/scale.hlsl").into()),
        },
        "scale",
        ShaderInput::Current,
        4,
    )?)
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_reports_compilation_missing_entry_and_backend_errors_without_fallback()
-> Result<()> {
    use crate::{Canvas, NativeRenderer, Radius};
    use peniko::kurbo::Rect;
    let context = context()?;
    let mut wrong_slot = coordinates()?.sources.as_ref().clone();
    wrong_slot.metal = wrong_slot
        .metal
        .map(|s| s.replace("[[buffer(10)]]", "[[buffer(11)]]"));
    wrong_slot.hlsl = wrong_slot
        .hlsl
        .map(|s| s.replace("register(t10)", "register(t11)"));
    for (sources, entry) in [
        (wrong_slot, "coordinates"),
        (
            ShaderSources {
                metal: Some("this cannot compile".into()),
                hlsl: Some("this cannot compile".into()),
            },
            "invalid",
        ),
        (coordinates()?.sources.as_ref().clone(), "missing"),
        (
            ShaderSources {
                metal: None,
                hlsl: Some("invalid".into()),
            },
            "invalid",
        ),
    ] {
        let program = ShaderProgram::new(sources, entry, ShaderInput::None, 0)?;
        let mut scene = Canvas::new(16, 16, 1.0);
        scene.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Radius::ZERO,
            program.brush([])?,
        );
        let mut renderer = NativeRenderer::with_context(&context, 16, 16)?;
        assert!(renderer.render_to_image(&scene).is_err());
    }
    // A failed shader must not poison subsequent frame uploads or pipeline reuse.
    let mut scene = Canvas::new(16, 16, 1.0);
    scene.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        Radius::ZERO,
        coordinates()?.brush(uniform(&[[0.01, 0.01, 0.0, 1.0]]))?,
    );
    let mut renderer = NativeRenderer::with_context(&context, 16, 16)?;
    assert_ne!(renderer.render_to_image(&scene)?.readback()?.pixels[0], 0);
    context.check_validation()?;
    Ok(())
}
