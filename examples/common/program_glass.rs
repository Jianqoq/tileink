//! Standalone application shaders: no generated effects or expression interpreter.
use tileink::{ShaderError, ShaderInput, ShaderProgram, ShaderSources};
pub fn program() -> Result<ShaderProgram, ShaderError> {
    ShaderProgram::new(
        ShaderSources {
            metal: Some(include_str!("../shaders/glass.metal").into()),
            hlsl: Some(include_str!("../shaders/glass.hlsl").into()),
        },
        "glass",
        ShaderInput::Sampled { radius: 32.0 },
        29,
    )
}
pub fn uniforms(center: [f32; 2], half_size: [f32; 2], radius: f32, dispersion: bool) -> Vec<u32> {
    let values = [
        [center[0], center[1], 0.0, 1.0],
        [half_size[0] - radius, half_size[1] - radius, 0.0, 0.0],
        [radius; 4],
        [1.0 / 24.0; 4],
        [-24.0, -24.0, 0.0, 0.0],
        [0.94, 0.97, 1.0, 1.0],
        [0.22, 0.24, 0.28, 0.0],
    ];
    let mut words: Vec<u32> = values.into_iter().flatten().map(f32::to_bits).collect();
    words.push(u32::from(dispersion));
    words
}
