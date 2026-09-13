#[allow(dead_code)]
#[path = "../build/gpu_constants.rs"]
mod gpu_constants;

#[test]
fn constants_resolve_hlsl_literals_aliases_and_products() {
    let constants = gpu_constants::parse("#pragma once\n// source\nstatic const uint TILE = 16u;\nstatic const uint LANES = TILE * TILE; // pixels\nstatic const uint ALIAS = LANES;\nstatic const uint MAXIMUM = 4294967295u;\n").unwrap();
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
