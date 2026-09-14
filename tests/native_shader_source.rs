#[allow(dead_code)]
#[path = "../build/gpu_constants.rs"]
mod gpu_constants;
#[path = "../build/native/source.rs"]
mod source;
use source::SourceGraph;
use std::fs;

#[test]
fn nested_include_contents_are_expanded_and_tracked() {
    let root = std::env::temp_dir().join(format!("tileink-shader-source-{}", std::process::id()));
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(
        root.join("main.hlsl"),
        "#include \"nested/shared.hlsli\"\nvoid main() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("nested/shared.hlsli"),
        "#include \"../abi.hlsli\"\n",
    )
    .unwrap();
    fs::write(root.join("abi.hlsli"), "static const uint stride = 16;\n").unwrap();
    let graph = SourceGraph::load(&root, "main.hlsl").unwrap();
    assert_eq!(graph.files.len(), 3);
    assert!(graph.expanded.contains("stride = 16"));
    assert!(!graph.expanded.contains("#include"));
    fs::write(root.join("abi.hlsli"), "static const uint stride = 32;\n").unwrap();
    assert_ne!(
        graph.expanded,
        SourceGraph::load(&root, "main.hlsl").unwrap().expanded
    );
    fs::write(root.join("abi.hlsli"), "#include \"main.hlsl\"\n").unwrap();
    assert!(
        SourceGraph::load(&root, "main.hlsl")
            .err()
            .unwrap()
            .to_string()
            .contains("cyclic")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_macro_and_escaping_includes_are_rejected() {
    let root = std::env::temp_dir().join(format!(
        "tileink-shader-source-invalid-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("source")).unwrap();
    fs::write(root.join("external.hlsli"), "uint hidden;").unwrap();
    for include in ["\"missing.hlsli\"", "SOME_MACRO", "\"../external.hlsli\""] {
        fs::write(
            root.join("source/main.hlsl"),
            format!("#include {include}\n"),
        )
        .unwrap();
        assert!(SourceGraph::load(&root.join("source"), "main.hlsl").is_err());
    }
    for directive in [
        "# include \"../external.hlsli\"",
        "#/**/include \"../external.hlsli\"",
        "#\\\ninclude \"../external.hlsli\"",
        "#define FILE \"../external.hlsli\"",
        "#if 0",
    ] {
        fs::write(root.join("source/main.hlsl"), directive).unwrap();
        assert!(SourceGraph::load(&root.join("source"), "main.hlsl").is_err());
    }
    fs::write(
        root.join("source/main.hlsl"),
        "/*\n#include \"absent\"\n*/\n// #include \"absent\"\nuint x;\n",
    )
    .unwrap();
    assert_eq!(
        SourceGraph::load(&root.join("source"), "main.hlsl")
            .unwrap()
            .files
            .len(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn include_guards_track_diamond_dependencies_and_guarded_self_includes() {
    let root = std::env::temp_dir().join(format!("tileink-shader-once-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("main.hlsl"),
        r#"#include "left.hlsli"
#include "right.hlsli"
"#,
    )
    .unwrap();
    fs::write(
        root.join("left.hlsli"),
        r#"#include "shared.hlsli"
"#,
    )
    .unwrap();
    fs::write(
        root.join("right.hlsli"),
        r#"#include "shared.hlsli"
"#,
    )
    .unwrap();
    fs::write(
        root.join("shared.hlsli"),
        r#"#ifndef SHARED_INCLUDED
#define SHARED_INCLUDED
#include "shared.hlsli"
static const uint STRIDE = 16u;
#endif
"#,
    )
    .unwrap();
    let graph = SourceGraph::load(&root, "main.hlsl").unwrap();
    assert_eq!(graph.files.len(), 4);
    assert_eq!(
        graph.expanded.matches("static const uint STRIDE").count(),
        1
    );
    assert!(!graph.expanded.contains("#ifndef"));
    fs::write(
        root.join("shared.hlsli"),
        "#ifndef SHARED_INCLUDED\n#define SHARED_INCLUDED\nstatic const uint STRIDE = 32u;\n#endif\n",
    )
    .unwrap();
    assert_ne!(
        graph.expanded,
        SourceGraph::load(&root, "main.hlsl").unwrap().expanded
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_and_reused_include_guards_are_rejected() {
    let root = std::env::temp_dir().join(format!("tileink-guard-invalid-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    for source in [
        "#ifndef HEADER\n#define OTHER\n#endif",
        "#ifndef HEADER\n#define HEADER\nuint x;",
        "#ifndef HEADER\n#endif",
    ] {
        fs::write(root.join("main.hlsl"), source).unwrap();
        assert!(SourceGraph::load(&root, "main.hlsl").is_err());
    }
    fs::write(
        root.join("a.hlsli"),
        "#ifndef SAME\n#define SAME\nuint a;\n#endif",
    )
    .unwrap();
    fs::write(
        root.join("b.hlsli"),
        "#ifndef SAME\n#define SAME\nuint b;\n#endif",
    )
    .unwrap();
    fs::write(
        root.join("main.hlsl"),
        r#"#include "a.hlsli"
#include "b.hlsli"
"#,
    )
    .unwrap();
    assert!(
        SourceGraph::load(&root, "main.hlsl")
            .err()
            .unwrap()
            .to_string()
            .contains("duplicate include guard")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn spirv_format_attribute_is_explicit_and_cannot_hide_shader_dependencies() {
    let root = std::env::temp_dir().join(format!("tileink-image-format-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let valid = "#ifdef __spirv__\n[[vk::image_format(\"rgba8\")]]\n#endif\nRWTexture2D<float4> target : register(u2);\n";
    fs::write(root.join("main.hlsl"), valid).unwrap();
    let graph = SourceGraph::load(&root, "main.hlsl").unwrap();
    assert!(graph.expanded.contains(valid));
    for invalid in [
        "#ifdef __spirv__\n#include \"hidden.hlsli\"\n#endif",
        "#ifdef __spirv__\nuint hidden;\n#endif",
        "#ifdef USER_FLAG\n[[vk::image_format(\"rgba8\")]]\n#endif",
        "#ifdef __spirv__\n[[vk::image_format(\"rgba8\")]]",
    ] {
        fs::write(root.join("main.hlsl"), invalid).unwrap();
        assert!(SourceGraph::load(&root, "main.hlsl").is_err());
    }
    fs::remove_dir_all(root).unwrap();
}
