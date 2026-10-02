#[allow(dead_code)]
#[path = "../build/native/dxc.rs"]
mod dxc;
#[allow(dead_code)]
#[path = "../build/gpu_constants.rs"]
mod gpu_constants;

#[test]
fn unavailable_compilers_and_unknown_targets_fail_explicitly() {
    assert!(dxc::Dxc::discover("relative/dxc.exe".into()).is_err());
    let missing = std::env::temp_dir()
        .join(format!("tileink-missing-dxc-{}", std::process::id()))
        .join("dxc.exe");
    assert!(dxc::Dxc::discover(missing).is_err());
    assert!(dxc::Dxc::flags("metal", "clear_words").is_err());
    for target in ["dxil", "spirv", "unknown"] {
        assert!(dxc::validate_container(target, b"not a shader").is_err());
    }
}

#[test]
#[ignore = "requires explicitly pinned TILEINK_NATIVE_DXC_PATH"]
fn pinned_dxc_reports_invalid_source_for_both_targets() {
    let compiler =
        dxc::Dxc::discover(std::env::var_os("TILEINK_NATIVE_DXC_PATH").unwrap().into()).unwrap();
    assert!(
        !compiler.identity["version"]
            .as_str()
            .unwrap()
            .trim()
            .is_empty()
    );
    let work = std::env::temp_dir().join(format!("tileink-dxc-negative-{}", std::process::id()));
    for target in ["dxil", "spirv"] {
        let flags = dxc::Dxc::flags(target, "clear_words").unwrap();
        let error = compiler
            .compile(
                "[numthreads(64,1,1)] void clear_words(){ undefined_name = 1; }",
                &flags,
                &work.join(target),
                target,
            )
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("undefined_name"), "{message}");
        assert!(message.contains(target), "{message}");
        assert!(!work.join(target).join(format!("shader.{target}")).exists());
    }
    std::fs::remove_dir_all(work).unwrap();
}

#[path = "../build/native/source.rs"]
mod source;

fn headers(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            files.extend(headers(&entry.path()));
        } else if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "hlsli")
        {
            files.push(entry.path());
        }
    }
    files.sort();
    files
}

#[test]
#[ignore = "requires explicitly pinned TILEINK_NATIVE_DXC_PATH"]
fn every_hlsl_header_compiles_without_callers_globals() {
    let compiler =
        dxc::Dxc::discover(std::env::var_os("TILEINK_NATIVE_DXC_PATH").unwrap().into()).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shaders/hlsl");
    let work = std::env::temp_dir().join(format!("tileink-header-contract-{}", std::process::id()));
    std::fs::create_dir(&work).unwrap();
    for (index, file) in headers(&root).iter().enumerate() {
        let name = file.strip_prefix(&root).unwrap().to_str().unwrap();
        let graph = source::SourceGraph::load(&root, name).unwrap();
        // A reusable header must declare its dependencies itself; no caller resource/config
        // declarations precede it. Actual entry tests separately validate argument wiring.
        let text = format!(
            "{}\n[numthreads(1,1,1)] void header_probe() {{}}",
            graph.expanded
        );
        for target in ["dxil", "spirv"] {
            compiler
                .compile(
                    &text,
                    &dxc::Dxc::flags(target, "header_probe").unwrap(),
                    &work.join(format!("{index}-{target}")),
                    target,
                )
                .unwrap_or_else(|error| panic!("{name} ({target}): {error}"));
        }
    }
    std::fs::remove_dir_all(work).unwrap();
}

#[test]
fn entry_resource_bindings_are_not_hidden_in_headers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shaders/hlsl");
    let mut guards = std::collections::BTreeSet::new();
    for file in headers(&root) {
        let name = file.strip_prefix(&root).unwrap().to_str().unwrap();
        let (_, guard) = hlsl_syntax::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert!(
            guards.insert(guard.expect("every header requires a conventional include guard")),
            "reused guard in {name}"
        );
        let graph = source::SourceGraph::load(&root, name).unwrap();
        assert!(
            !graph
                .expanded
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .any(|token| token == "register"),
            "entry resource binding hidden in {name}"
        );
    }
}

use gpu_constants::syntax as hlsl_syntax;

// Shader Tools reserves these type names even where DXC accepts them as identifiers.
// Check declaration positions rather than banning legitimate matrix/vector types.
fn reserved_declaration(source: &str) -> Option<&str> {
    let tokens: Vec<_> = source
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '>')
        .filter(|token| !token.is_empty())
        .collect();
    tokens.windows(2).find_map(|pair| {
        let typed = pair[0].ends_with('>')
            || ["float", "half", "double", "int", "uint", "bool"]
                .iter()
                .any(|base| {
                    pair[0]
                        .strip_prefix(base)
                        .is_some_and(|tail| tail.chars().all(|c| c.is_ascii_digit() || c == 'x'))
                });
        (typed && matches!(pair[1], "matrix" | "vector" | "texture")).then_some(pair[1])
    })
}

#[test]
fn hlsl_declarations_avoid_editor_type_keywords() {
    assert_eq!(
        reserved_declaration("float4 matrix = asfloat(bits);"),
        Some("matrix")
    );
    assert_eq!(
        reserved_declaration("Texture2D<float4> texture,"),
        Some("texture")
    );
    assert_eq!(reserved_declaration("float3 vector;"), Some("vector"));
    assert_eq!(
        reserved_declaration("matrix<float, 4, 4> transform; float4 affine_linear;"),
        None
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shaders/hlsl");
    for file in headers(&root) {
        let name = file.strip_prefix(&root).unwrap().to_str().unwrap();
        let graph = source::SourceGraph::load(&root, name).unwrap();
        assert_eq!(reserved_declaration(&graph.expanded), None, "{name}");
    }
}

#[test]
fn hlsl_blend_modes_match_serialized_scene_modes() {
    use peniko::{Compose, Mix};
    let constants =
        gpu_constants::parse(include_str!("../src/shaders/hlsl/shared/blend/modes.hlsli")).unwrap();
    let expected = [
        ("MIX_NORMAL", Mix::Normal as u32),
        ("MIX_MULTIPLY", Mix::Multiply as u32),
        ("MIX_SCREEN", Mix::Screen as u32),
        ("MIX_OVERLAY", Mix::Overlay as u32),
        ("MIX_DARKEN", Mix::Darken as u32),
        ("MIX_LIGHTEN", Mix::Lighten as u32),
        ("MIX_COLOR_DODGE", Mix::ColorDodge as u32),
        ("MIX_COLOR_BURN", Mix::ColorBurn as u32),
        ("MIX_HARD_LIGHT", Mix::HardLight as u32),
        ("MIX_SOFT_LIGHT", Mix::SoftLight as u32),
        ("MIX_DIFFERENCE", Mix::Difference as u32),
        ("MIX_EXCLUSION", Mix::Exclusion as u32),
        ("MIX_HUE", Mix::Hue as u32),
        ("MIX_SATURATION", Mix::Saturation as u32),
        ("MIX_COLOR", Mix::Color as u32),
        ("MIX_LUMINOSITY", Mix::Luminosity as u32),
        ("COMPOSE_CLEAR", Compose::Clear as u32),
        ("COMPOSE_COPY", Compose::Copy as u32),
        ("COMPOSE_DEST", Compose::Dest as u32),
        ("COMPOSE_SRC_OVER", Compose::SrcOver as u32),
        ("COMPOSE_DEST_OVER", Compose::DestOver as u32),
        ("COMPOSE_SRC_IN", Compose::SrcIn as u32),
        ("COMPOSE_DEST_IN", Compose::DestIn as u32),
        ("COMPOSE_SRC_OUT", Compose::SrcOut as u32),
        ("COMPOSE_DEST_OUT", Compose::DestOut as u32),
        ("COMPOSE_SRC_ATOP", Compose::SrcAtop as u32),
        ("COMPOSE_DEST_ATOP", Compose::DestAtop as u32),
        ("COMPOSE_XOR", Compose::Xor as u32),
        ("COMPOSE_PLUS", Compose::Plus as u32),
        ("COMPOSE_PLUS_LIGHTER", Compose::PlusLighter as u32),
    ];
    assert_eq!(constants.len(), expected.len());
    for (name, value) in expected {
        assert_eq!(constants[name], value, "{name}");
    }
}
