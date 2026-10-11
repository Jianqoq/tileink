use super::*;
use crate::{ShaderInput, ShaderProgram, ShaderSources};
#[test]
fn pipeline_cache_population_does_not_change_execution_plan_fingerprint() {
    use crate::{ShaderBinding, ShaderBindingKind, ShaderKernel, ShaderTextureFormat};
    let kernel = ShaderKernel::new(
        ShaderSources {
            metal: Some("kernel void copy() {}".into()),
            hlsl: None,
        },
        "copy",
        [8, 8, 1],
        vec![ShaderBinding {
            name: "output".into(),
            slot: 3,
            kind: ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
        }],
    )
    .unwrap();
    let before = format!("{kernel:?}");
    let a = pipeline::source(&kernel);
    let b = pipeline::source(&kernel.clone());
    assert!(std::rc::Rc::ptr_eq(&a, &b));
    assert_eq!(before, format!("{kernel:?}"));
}
#[test]
fn shader_compilation_receives_exact_user_source_and_cache_excludes_instance_data() {
    let text = "// verbatim source\n#include <metal_stdlib>\nkernel void example() {}\n";
    let program = ShaderProgram::new(
        ShaderSources {
            metal: Some(text.into()),
            hlsl: None,
        },
        "example",
        ShaderInput::None,
        1,
    )
    .unwrap();
    let first = program.brush([1]).unwrap();
    let second = first.with_uniforms([u32::MAX]).unwrap();
    let a = pipeline::source(&first.0.program.kernel);
    let b = pipeline::source(&second.0.program.kernel);
    assert_eq!(a.text(true).unwrap().as_bytes(), text.as_bytes());
    assert_eq!(a.key, b.key);
    assert_eq!(&*a.entry, "example");
    assert!(a.text(false).is_err());
    let changed = ShaderProgram::new(
        ShaderSources {
            metal: Some(format!("{text}\n")),
            hlsl: None,
        },
        "example",
        ShaderInput::None,
        1,
    )
    .unwrap();
    assert_ne!(a.key, pipeline::source(&changed.kernel).key);
}

#[test]
fn source_cache_keys_distinguish_standard_config_from_raw_constant_blocks() {
    use crate::{ShaderBinding, ShaderBindingKind as K, ShaderKernel, ShaderTextureFormat};
    let make = |kind| {
        ShaderKernel::new(
            ShaderSources {
                metal: Some("source".into()),
                hlsl: None,
            },
            "main",
            [8, 8, 1],
            vec![
                ShaderBinding {
                    name: "config".into(),
                    slot: 0,
                    kind,
                },
                ShaderBinding {
                    name: "output".into(),
                    slot: 3,
                    kind: K::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
                },
            ],
        )
        .unwrap()
    };
    assert_ne!(
        pipeline::source(&make(K::Config)).key,
        pipeline::source(&make(K::Uniform(112))).key
    );
}
