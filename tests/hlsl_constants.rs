#[allow(dead_code)]
#[path = "../build/gpu_constants.rs"]
mod gpu_constants;

#[test]
fn constants_resolve_hlsl_literals_aliases_and_products() {
    let constants = gpu_constants::parse("// source\nstatic const uint TILE = 16u;\nstatic const uint LANES = TILE * TILE; // pixels\nstatic const uint ALIAS = LANES;\nstatic const uint MAXIMUM = 4294967295u;\n").unwrap();
    assert_eq!(constants["TILE"], 16);
    assert_eq!(constants["LANES"], 256);
    assert_eq!(constants["ALIAS"], 256);
    assert_eq!(constants["MAXIMUM"], u32::MAX);
}

#[test]
fn constants_reject_ambiguous_or_unsupported_definitions() {
    for source in [
        "",
        "// empty",
        "#define SIZE 16",
        "static const uint SIZE = 16",
        "static const uint lower = 1;",
        "static const uint SIZE = UNKNOWN;",
        "static const uint SIZE = 4294967296u;",
        "static const uint SIZE = 65536u * 65536u;",
        "static const uint SIZE = -1;",
        "static const uint SIZE = 1.5;",
        "static const uint SIZE = 016u;",
        "static const uint SIZE = 1 + 2;",
        "static const uint SIZE = 1u;\nstatic const uint SIZE = 2u;",
        "static const uint SIZE = ;",
    ] {
        assert!(gpu_constants::parse(source).is_err(), "{source}");
    }
}

#[test]
fn float_constants_preserve_literals_and_reject_ambiguous_input() {
    let source = "static const float EDGE = -0.001;\nstatic const float EPS = 1.0e-6;\n";
    assert_eq!(
        gpu_constants::floats::parse_wgsl(source).unwrap(),
        "const EDGE: f32 = -0.001;\nconst EPS: f32 = 1.0e-6;\n"
    );
    for source in [
        "",
        "static const float VALUE = NaN;",
        "static const float VALUE = 1e99;",
        "static const float VALUE = 1.0 + 2.0;",
        "static const float value = 1.0;",
        "static const float VALUE = 1.0;\nstatic const float VALUE = 2.0;",
        "static const float VALUE = 1;",
        "static const float VALUE = +1.0;",
    ] {
        assert!(
            gpu_constants::floats::parse_wgsl(source).is_err(),
            "{source}"
        );
    }
}
