#![cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../build/native/cache.rs"]
mod cache;
#[allow(dead_code)]
#[path = "../build/native/metal.rs"]
mod compiler;
#[allow(dead_code)]
#[path = "../build/gpu_constants.rs"]
mod gpu_constants;
#[path = "../build/native/source.rs"]
mod source;

#[test]
fn missing_tools_and_invalid_libraries_are_rejected() {
    assert!(compiler::MetalCompiler::discover("xcrun".into()).is_err());
    assert!(compiler::MetalCompiler::discover("/nonexistent/tileink/xcrun".into()).is_err());
    for bytes in [b"".as_slice(), b"MTLB", b"DXBCabcdefghijklmnop"] {
        assert!(compiler::validate_container(bytes).is_err());
    }
}

#[test]
fn metal_includes_and_target_identity_invalidate_the_common_cache() {
    let root = tempfile::tempdir().unwrap();
    let source = "#include <metal_stdlib>\n#include \"params.metal\"\nkernel void clear_words() {}";
    std::fs::write(root.path().join("main.metal"), source).unwrap();
    assert!(source::SourceGraph::load(root.path(), "main.metal").is_err());
    std::fs::write(
        root.path().join("params.metal"),
        "struct Params { uint count; };\n",
    )
    .unwrap();
    let graph = source::SourceGraph::load(root.path(), "main.metal").unwrap();
    assert_eq!(graph.files.len(), 2);
    let recipe = [
        "msl",
        "macos-metallib",
        "sdk-15.2",
        "toolchain-digest",
        "abi-v1",
        "metal2.4",
        "no-fast-math",
    ];
    let key = |recipe: &[&str], source: &str| {
        cache::CacheKey::new(&[&serde_json::to_vec(recipe).unwrap(), source.as_bytes()])
    };
    let initial = key(&recipe, &graph.expanded);
    for index in 0..recipe.len() {
        let mut changed = recipe;
        changed[index] = "changed";
        assert_ne!(initial, key(&changed, &graph.expanded));
    }
    std::fs::write(
        root.path().join("params.metal"),
        "struct Params { uint count; uint stride; };\n",
    )
    .unwrap();
    assert_ne!(
        initial,
        key(
            &recipe,
            &source::SourceGraph::load(root.path(), "main.metal")
                .unwrap()
                .expanded
        )
    );
    let cache = cache::ShaderCache::new(root.path().join("cache"));
    cache
        .get_or_compile(&initial, || {
            Ok(b"corrupt-but-correctly-framed-library".to_vec())
        })
        .unwrap();
    let rebuilt = cache
        .get_or_compile_validated(
            &initial,
            |bytes| Ok(compiler::validate_container(bytes).is_ok()),
            || Ok(b"MTLB0123456789012345".to_vec()),
        )
        .unwrap();
    assert!(!rebuilt.hit);
}

#[test]
#[ignore = "requires Xcode Metal tools"]
fn apple_compiler_builds_independent_msl_and_rejects_invalid_source() {
    let compiler = compiler::MetalCompiler::discover("/usr/bin/xcrun".into()).unwrap();
    let graph = source::SourceGraph::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shaders/metal"),
        "probes.metal",
    )
    .unwrap();
    let work = tempfile::tempdir().unwrap();
    let bytes = compiler.compile(&graph.expanded, work.path()).unwrap();
    compiler::validate_container(&bytes).unwrap();
    let error = compiler
        .compile(
            "#include <metal_stdlib>\nkernel void broken() { undefined_symbol = 1; }",
            work.path(),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("undefined_symbol"), "{error}");
    for flag in compiler::FLAGS {
        assert!(error.contains(flag), "missing compile flag {flag}: {error}");
    }
    assert!(!work.path().join("shader.metallib").exists());
}
