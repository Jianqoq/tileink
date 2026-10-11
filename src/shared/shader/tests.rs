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
fn brush_encoding_contains_only_source_uniforms_and_transform() {
    use crate::shared::brush::ENCODED_BRUSH_HEADER_WORDS;
    for sources in [
        ShaderSources {
            metal: Some("// 颜色\nkernel void paint() {}".into()),
            hlsl: None,
        },
        ShaderSources {
            metal: None,
            hlsl: Some("// HLSL\nvoid paint() {}".into()),
        },
        ShaderSources {
            metal: Some("metal".into()),
            hlsl: Some("hlsl".into()),
        },
    ] {
        for uniforms in [&[][..], &[0, 0x7fc00000, u32::MAX][..]] {
            let brush =
                ShaderProgram::new(sources.clone(), "paint", ShaderInput::None, uniforms.len())
                    .unwrap()
                    .brush(uniforms)
                    .unwrap();
            let mut expected = vec![uniforms.len() as u32];
            for source in [
                Some("paint"),
                sources.metal.as_deref(),
                sources.hlsl.as_deref(),
            ] {
                let bytes = source.unwrap_or_default().as_bytes();
                expected.push(bytes.len() as u32);
                for chunk in bytes.chunks(4) {
                    let mut word = [0; 4];
                    word[..chunk.len()].copy_from_slice(chunk);
                    expected.push(u32::from_le_bytes(word));
                }
            }
            expected.extend(uniforms);
            let mut blob = vec![0xfeedface; 7];
            let (offset, len) = brush.0.encode(&mut blob);
            assert_eq!(offset, 7);
            assert_eq!(&blob[..7], &[0xfeedface; 7]);
            assert_eq!(&blob[7 + ENCODED_BRUSH_HEADER_WORDS..], expected);
            assert_eq!(len as usize, ENCODED_BRUSH_HEADER_WORDS + expected.len());
            assert_eq!(brush.0.word_len(), len as usize);
            let crate::Brush::Program(decoded) =
                crate::shared::brush::decode_encoded_brush(&blob, offset, len).unwrap()
            else {
                panic!("canonical native brush retains its kind");
            };
            assert_eq!(*decoded.0.program.sources, sources);
            assert_eq!(decoded.0.program.input, ShaderInput::None);
            assert_eq!(decoded.0.uniforms.as_ref(), uniforms);
            assert_eq!(decoded.0.transform, brush.0.transform);
        }
    }
}

#[test]
fn brush_decode_rejects_retired_input_metadata_and_malformed_payloads() {
    use crate::shared::brush::ENCODED_BRUSH_HEADER_WORDS;
    let brush = program(ShaderInput::None, 1).brush([0x7fc00000]).unwrap();
    let mut words = Vec::new();
    brush.0.encode(&mut words);
    let payload = &words[ENCODED_BRUSH_HEADER_WORDS..];
    let decode = |p: &[u32]| BrushInstance::decode(p, brush.0.transform);
    assert!(decode(payload).is_some());
    let mut retired = payload.to_vec();
    retired.splice(1..1, [0, 0]);
    assert!(decode(&retired).is_none());
    for (index, value) in [(0, u32::MAX), (1, u32::MAX), (2, u32::MAX)] {
        let mut malformed = payload.to_vec();
        malformed[index] = value;
        assert!(decode(&malformed).is_none());
    }
    let mut trailing = payload.to_vec();
    trailing.push(0);
    assert!(decode(&trailing).is_none());
    for value in [f32::NAN, f32::INFINITY] {
        let mut transform = brush.0.transform;
        transform[0] = value;
        assert!(BrushInstance::decode(payload, transform).is_none());
    }
}
#[test]
fn shader_source_and_instance_roundtrip_without_changing_bytes() {
    let mut brush = program(ShaderInput::None, 3)
        .brush([0, 0x7fc00000, u32::MAX])
        .unwrap();
    brush.0.transform = [0.5, 0.0, 0.0, 0.5, -2.0, -3.0];
    let mut words = Vec::new();
    let (_, len) = brush.0.encode(&mut words);
    let decoded = BrushInstance::decode(&words[21..], brush.0.transform).unwrap();
    assert_eq!(*decoded.program.sources, *brush.0.program.sources);
    assert_eq!(decoded.uniforms, brush.0.uniforms);
    let mut second = Vec::new();
    decoded.encode(&mut second);
    assert_eq!(words, second);
    assert_eq!(len as usize, words.len());
    assert!(BrushInstance::decode(&words[21..words.len() - 1], brush.0.transform).is_none());
    for prefix in 0..words.len() - 21 {
        assert!(BrushInstance::decode(&words[21..21 + prefix], brush.0.transform).is_none());
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
        program(ShaderInput::None, 2).brush([1]),
        Err(ShaderError::UniformCount)
    ));
    assert!(matches!(
        ShaderProgram::new(
            (*program(ShaderInput::None, 0).sources).clone(),
            "paint",
            ShaderInput::None,
            usize::MAX
        ),
        Err(ShaderError::InvalidUniformSize)
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
