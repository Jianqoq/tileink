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
