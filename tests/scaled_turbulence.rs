#[path = "support/native_gpu.rs"]
mod backend;
#[allow(unused_imports)]
#[path = "../examples/common/mod.rs"]
mod common;

#[test]
#[ignore = "requires pinned TILEINK_TEST_GPU; run each backend separately"]
fn scaled_turbulence_matches_canonical_rgba() {
    use sha2::{Digest, Sha256};
    let mut gpu = backend::Gpu::new();
    // Both sizes share the same SVG and noise parameters. The large output
    // exercises half-channel boundaries missed by the normal SVG-size corpus.
    for (width, expected) in [
        (
            300,
            "5ef9ecee9f6f13992812b4fad850a8718bb32b7070eaece93c0888494d279ce4",
        ),
        (
            1600,
            "759888711bfa14070fd01c6640d6be0ac41d65b96942784475110034506caf82",
        ),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/svg/tests/filters/feTurbulence/numOctaves=5.svg");
        let (canvas, width, height) = common::load_svg_scene(path, width).unwrap();
        let target = gpu.target(width, height);
        gpu.render_immediate(&canvas, &target);
        let image = gpu.image_target(&target);
        assert_eq!((image.width, image.height), (width, height));
        let bytes: Vec<_> = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_le_bytes())
            .collect();
        assert_eq!(
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            expected,
            "{width}px turbulence differs from four-route canonical pixels"
        );
    }
}
