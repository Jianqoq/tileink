#![cfg(all(target_os = "macos", feature = "wgpu"))]
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[path = "../src/native/runtime/program/probe.rs"]
mod program;
use program::{Params, Probe};
#[allow(dead_code)]
#[path = "../build/native/cache.rs"]
mod cache;
#[path = "../src/native/runtime/tests/cases.rs"]
mod cases;
#[path = "../build/native/metal.rs"]
mod compiler;
#[allow(dead_code)]
#[path = "../build/gpu_constants.rs"]
mod gpu_constants;
#[path = "metal/native.rs"]
mod native;
#[path = "metal/reference.rs"]
mod reference;
#[path = "../build/native/source.rs"]
mod source;

#[allow(dead_code)]
#[path = "../build/native/abi.rs"]
mod abi;
#[allow(dead_code)]
#[path = "../build/native/interfaces.rs"]
mod interfaces;

#[test]
#[ignore = "requires Xcode tools and a physical Metal GPU"]
fn native_msl_matches_wgpu_metal_and_cpu_byte_for_byte() -> Result<()> {
    use objc2_metal::MTLDevice;
    let compiler = compiler::MetalCompiler::discover("/usr/bin/xcrun".into())?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let graph = source::SourceGraph::load(&root.join("src/shaders/metal"), "probes.metal")?;
    let recipe = serde_json::json!({"schema":1,"language":"msl","target":"macos-metallib",
        "flags":compiler::FLAGS,"toolchain":compiler.identity,"sources":graph.files,
        "abi_sha256":compiler::digest(&interfaces::get("probe")?.cache_bytes())});
    let key = cache::CacheKey::new(&[&serde_json::to_vec(&recipe)?, graph.expanded.as_bytes()]);
    let work = tempfile::tempdir()?;
    let artifacts = cache::ShaderCache::new(root.join("target/tileink-metal-shaders"));
    let artifact = artifacts.get_or_compile_validated(
        &key,
        |bytes| Ok(compiler::validate_container(bytes).is_ok()),
        || compiler.compile(&graph.expanded, work.path()),
    )?;
    let native = native::Native::new(&artifact.bytes)?;
    // Deliberately valid metallibs with the wrong ABI must fail at reflection,
    // before submitting any GPU work. Container framing alone cannot detect this.
    for corrupted in [
        graph.expanded.replace(
            "uint source_offset;\n    uint destination_offset;",
            "uint destination_offset;\n    uint source_offset;",
        ),
        graph.expanded.replace("[[buffer(2)]]", "[[buffer(4)]]"),
    ] {
        assert_ne!(corrupted, graph.expanded);
        let bytes = compiler.compile(&corrupted, work.path())?;
        assert!(
            native::Native::new(&bytes).is_err(),
            "wrong Metal ABI accepted"
        );
    }
    let identity = native.device.registryID();
    let reference = reference::Reference::new(identity)?;
    let cases = cases::cases();
    assert_eq!(cases.len(), 306, "probe manifest changed; review coverage");
    let mut frames = Vec::new();
    for repetition in 0..3 {
        for (index, case) in cases.iter().enumerate() {
            objc2::rc::autoreleasepool(|_| -> Result<()> {
                let actual = native.execute(&case.dispatch)?;
                let expected = reference.execute(case)?;
                assert_eq!(
                    actual, case.expected,
                    "Metal vs CPU: case {index}, repetition {repetition}"
                );
                assert_eq!(
                    actual, expected,
                    "Metal vs wgpu: case {index}, repetition {repetition}"
                );
                frames.push(serde_json::json!({"case":index,"repetition":repetition,"entry":case.entry,"bytes":actual.len(),"sha256":compiler::digest(&actual),"different_bytes":0}));
                Ok(())
            })?;
        }
    }
    let system = std::process::Command::new("/usr/bin/sw_vers").output()?;
    if !system.status.success() {
        return Err("cannot record macOS identity".into());
    }
    let report = serde_json::json!({"device":native.device.name().to_string(),"registry_id":format!("{identity:016x}"),"system":String::from_utf8(system.stdout)?,"cases":cases.len(),"repetitions":3,"routes":["native-metal","wgpu-metal","cpu"],"cache_key":key.hex(),"artifact_sha256":compiler::digest(&artifact.bytes),"recipe":recipe,"frames":frames,
        "validation_requested": std::env::var("MTL_DEBUG_LAYER").ok().as_deref() == Some("1"),
        "cases_sha256":compiler::digest(include_bytes!("../src/native/runtime/tests/cases.rs")),
        "reference_sha256":compiler::digest(include_bytes!("../src/native/runtime/tests/reference.wgsl"))});
    if let Some(path) = std::env::var_os("TILEINK_METAL_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    }
    eprintln!(
        "Metal: 306 cases × 3 repetitions; native MSL, wgpu-Metal and CPU bytes equal; registry {identity:016x}"
    );
    Ok(())
}
