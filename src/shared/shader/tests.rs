use super::*;
fn program(input: ShaderInput, words: usize) -> ShaderProgram {
    ShaderProgram::new(
        ShaderSources {
            metal: Some("// preserve this source\nkernel void paint() {}".into()),
            hlsl: Some("// hlsl\nvoid paint() {}".into()),
        },
        "paint",
        input,
        words,
    )
    .unwrap()
}
#[test]
fn shader_source_and_instance_roundtrip_without_changing_bytes() {
    let mut brush = program(ShaderInput::None, 3)
        .brush([0, 0x7fc00000, u32::MAX])
        .unwrap();
    brush.0.transform = [0.5, 0.0, 0.0, 0.5, -2.0, -3.0];
    let mut words = Vec::new();
    let (_, len) = brush.0.encode(&mut words);
    let decoded = ProgramInstance::decode(&words[21..], brush.0.transform, false).unwrap();
    assert_eq!(*decoded.program.sources, *brush.0.program.sources);
    assert_eq!(decoded.uniforms, brush.0.uniforms);
    let mut second = Vec::new();
    decoded.encode(&mut second);
    assert_eq!(words, second);
    assert_eq!(len as usize, words.len());
    assert!(
        ProgramInstance::decode(&words[21..words.len() - 1], brush.0.transform, false).is_none()
    );
    for prefix in 0..words.len() - 21 {
        assert!(
            ProgramInstance::decode(&words[21..21 + prefix], brush.0.transform, false).is_none()
        );
    }
}
#[test]
fn shader_validates_sources_entry_uniform_size_and_input_declaration() {
    for sources in [
        ShaderSources::default(),
        ShaderSources {
            metal: Some("  ".into()),
            hlsl: None,
        },
    ] {
        assert!(matches!(
            ShaderProgram::new(sources, "paint", ShaderInput::None, 0),
            Err(ShaderError::MissingSource)
        ));
    }
    for entry in ["", "123", "x-y", "x\0", "绘制"] {
        assert!(matches!(
            ShaderProgram::new(
                (*program(ShaderInput::None, 0).sources).clone(),
                entry,
                ShaderInput::None,
                0
            ),
            Err(ShaderError::InvalidEntry)
        ));
    }
    for radius in [-1.0, f32::NAN, f32::INFINITY] {
        assert!(matches!(
            ShaderProgram::new(
                (*program(ShaderInput::None, 0).sources).clone(),
                "paint",
                ShaderInput::Sampled { radius },
                0
            ),
            Err(ShaderError::InvalidSampleRadius)
        ));
    }
    assert!(matches!(
        program(ShaderInput::None, 2).filter([1]),
        Err(ShaderError::UniformCount)
    ));
    assert!(matches!(
        program(ShaderInput::Current, 0).brush([]),
        Err(ShaderError::BrushReadsInput)
    ));
    assert!(matches!(
        program(ShaderInput::Sampled { radius: 0.0 }, 0).brush([]),
        Err(ShaderError::BrushReadsInput)
    ));
    let filter = program(ShaderInput::Current, 0).filter([]).unwrap();
    assert!(filter.reads_input());
    assert_eq!(filter.sample_outset(), Some(0));
}
#[test]
fn shader_sampling_declaration_preserves_transformed_dependencies_without_output_outset() {
    use crate::shared::layer::filter::{Filter, filter_dependency, filter_outset};
    let mut filter = program(ShaderInput::Sampled { radius: 3.5 }, 0)
        .filter([])
        .unwrap();
    assert_eq!(filter.sample_outset(), Some(5));
    filter.transform = [0.5, 0.0, 0.0, 0.5, 0.0, 0.0];
    assert_eq!(filter.sample_outset(), Some(8));
    let filter = Filter::from(filter);
    assert_eq!(filter_outset(&filter), 0);
    assert_eq!(filter_dependency(&filter).local_radius(), Some(8));
}
