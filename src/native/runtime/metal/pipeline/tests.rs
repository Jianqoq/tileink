use super::*;

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn failed_user_shader_does_not_accumulate_libraries() -> Result<()> {
    let metal = super::super::Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        physical_adapter: Some(std::env::var("TILEINK_TEST_GPU")?),
    })?;
    let shader = crate::native::shaders::SHADER_ARTIFACTS
        .iter()
        .find(|s| s.entry == "copy_words")
        .unwrap();
    let mut libraries = BTreeMap::new();
    let mut pipelines = BTreeMap::new();
    for key in ["program:missing-a", "program:missing-b"] {
        assert!(
            ensure_inner(
                &metal.device,
                &mut libraries,
                &mut pipelines,
                *shader,
                key,
                "missing",
                Some("#include <metal_stdlib>\nkernel void present() {}"),
            )
            .is_err()
        );
        assert!(libraries.is_empty(), "failed source remained cached");
        assert!(pipelines.is_empty());
    }
    Ok(())
}
