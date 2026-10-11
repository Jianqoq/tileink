//! The fixed pixel ABI is a convenience frontend for ordinary public pipelines.
use super::super::ShaderProgram;
use super::*;
pub(in crate::shared::shader) fn kernel(
    sources: Rc<ShaderSources>,
    entry: &str,
    input: ShaderInput,
) -> Result<ShaderKernel, ShaderError> {
    use ShaderBindingKind as K;
    let mut bindings = vec![
        ("config", 0, K::Config),
        (
            "output_image",
            3,
            K::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
        ),
        ("active_tiles", 8, K::ReadBuffer),
        ("uniforms", 10, K::ReadBuffer),
    ];
    if input != ShaderInput::None {
        bindings.push(("input_image", 1, K::Texture));
    }
    ShaderKernel::new(
        sources,
        entry,
        [256, 1, 1],
        bindings
            .into_iter()
            .map(|(name, slot, kind)| ShaderBinding {
                name: name.into(),
                slot,
                kind,
            })
            .collect(),
    )
}
pub(in crate::shared::shader) fn pipeline(
    program: &ShaderProgram,
    uniforms: &[u32],
) -> Result<ShaderPipeline, ShaderError> {
    let mut resources = vec![
        ShaderResource::Config(ShaderConfigDescriptor {
            extent: ShaderExtent::FULL,
            sample_radius: program.sample_radius(),
            uniform_words: program.uniform_words as u32,
        }),
        ShaderResource::Output,
        ShaderResource::ActiveTiles,
        ShaderResource::Buffer(if uniforms.is_empty() {
            Rc::from([0])
        } else {
            uniforms.into()
        }),
    ];
    let mut bindings = vec![(0, 0), (3, 1), (8, 2), (10, 3)];
    if program.reads_input() {
        resources.push(ShaderResource::Input(0));
        bindings.push((1, 4));
    }
    ShaderPipeline::new(
        resources,
        vec![ShaderPass {
            kernel: program.kernel.clone(),
            bindings,
            dispatch: ShaderDispatch::Pixels,
        }],
    )?
    .with_input_dependency(program.input)
}
